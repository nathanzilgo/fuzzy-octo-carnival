
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum PomodoroState {
    Idle,
    Running,
    Paused,
    Finished,
}

#[derive(Clone)]
struct PomodoroSession {
    id: u64,
    work_minutes: u64,
    break_minutes: u64,
    state: PomodoroState,
    started_at: Option<Instant>,
    paused_at: Option<Instant>,
    elapsed_secs: u64,
}

impl PomodoroSession {
    fn new(id: u64, work_minutes: u64, break_minutes: u64) -> Self {
        Self {
            id,
            work_minutes,
            break_minutes,
            state: PomodoroState::Idle,
            started_at: None,
            paused_at: None,
            elapsed_secs: 0,
        }
    }

    fn total_work_secs(&self) -> u64 {
        self.work_minutes * 60
    }

    fn update_elapsed(&mut self) {
        if let (PomodoroState::Running, Some(start)) = (self.state, self.started_at) {
            let now = Instant::now();
            self.elapsed_secs += now.saturating_duration_since(start).as_secs();
            self.started_at = Some(now);
            if self.elapsed_secs >= self.total_work_secs() {
                self.state = PomodoroState::Finished;
            }
        }
    }

    fn remaining_secs(&mut self) -> u64 {
        self.update_elapsed();
        self.total_work_secs().saturating_sub(self.elapsed_secs)
    }
}

#[derive(Default, Clone)]
struct AppState {
    next_id: u64,
    sessions: HashMap<u64, PomodoroSession>,
}

type SharedState = Arc<Mutex<AppState>>;

#[derive(Deserialize)]
struct CreateSessionReq {
    work_minutes: u64,
    break_minutes: u64,
}

#[derive(Serialize)]
struct SessionResponse {
    id: u64,
    work_minutes: u64,
    break_minutes: u64,
    state: PomodoroState,
    elapsed_secs: u64,
    remaining_secs: u64,
}

fn to_response(mut s: PomodoroSession) -> SessionResponse {
    let remaining = s.remaining_secs();
    SessionResponse {
        id: s.id,
        work_minutes: s.work_minutes,
        break_minutes: s.break_minutes,
        state: s.state,
        elapsed_secs: s.elapsed_secs,
        remaining_secs: remaining,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let state: SharedState = Arc::new(Mutex::new(AppState::default()));

    let app = Router::new()
        .route("/sessions", post(create_session).get(list_sessions))
        .route(
            "/sessions/:id/start",
            post(start_session),
        )
        .route(
            "/sessions/:id/pause",
            post(pause_session),
        )
        .route(
            "/sessions/:id/resume",
            post(resume_session),
        )
        .route(
            "/sessions/:id",
            get(get_session),
        )
        .with_state(state);

    let listener = TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn create_session(
    State(state): State<SharedState>,
    Json(req): Json<CreateSessionReq>,
) -> (StatusCode, Json<SessionResponse>) {
    let mut guard = state.lock().unwrap();
    guard.next_id += 1;
    let id = guard.next_id;
    let session = PomodoroSession::new(id, req.work_minutes, req.break_minutes);
    guard.sessions.insert(id, session.clone());
    (
        StatusCode::CREATED,
        Json(to_response(session)),
    )
}

async fn list_sessions(
    State(state): State<SharedState>,
) -> Json<Vec<SessionResponse>> {
    let mut guard = state.lock().unwrap();
    let res = guard
        .sessions
        .values()
        .cloned()
        .map(to_response)
        .collect();
    Json(res)
}

async fn get_session(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<Json<SessionResponse>, StatusCode> {
    let mut guard = state.lock().unwrap();
    let session = guard.sessions.get(&id).cloned().ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(to_response(session)))
}

async fn start_session(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<Json<SessionResponse>, StatusCode> {
    let mut guard = state.lock().unwrap();
    let s = guard.sessions.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;
    if s.state == PomodoroState::Idle || s.state == PomodoroState::Finished {
        s.elapsed_secs = 0;
        s.started_at = Some(Instant::now());
        s.paused_at = None;
        s.state = PomodoroState::Running;
    }
    Ok(Json(to_response(s.clone())))
}

async fn pause_session(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<Json<SessionResponse>, StatusCode> {
    let mut guard = state.lock().unwrap();
    let s = guard.sessions.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;
    if s.state == PomodoroState::Running {
        s.update_elapsed();
        s.state = PomodoroState::Paused;
    }
    Ok(Json(to_response(s.clone())))
}

async fn resume_session(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<Json<SessionResponse>, StatusCode> {
    let mut guard = state.lock().unwrap();
    let s = guard.sessions.get_mut(&id).ok_or(StatusCode::NOT_FOUND)?;
    if s.state == PomodoroState::Paused {
        s.started_at = Some(Instant::now());
        s.paused_at = None;
        s.state = PomodoroState::Running;
    }
    Ok(Json(to_response(s.clone())))
}
