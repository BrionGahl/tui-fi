use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender as StdSender;
use std::thread;

use mpris_server::{Metadata, PlaybackStatus, Player, Time};

use crate::player::Player as AudioPlayer;

pub enum Cmd {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
}

#[derive(Clone, Default)]
struct State {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    length_us: Option<i64>,
    paused: bool,
    stopped: bool,
}

pub struct MprisHandle {
    state: Arc<Mutex<State>>,
    tx: tokio::sync::mpsc::Sender<()>,
}

impl MprisHandle {
    pub fn spawn(cmd_tx: StdSender<Cmd>) -> Option<Self> {
        let state: Arc<Mutex<State>> = Arc::new(Mutex::new(State::default()));
        let (tx, rx) = tokio::sync::mpsc::channel::<()>(8);

        let state_clone = state.clone();
        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(run(state_clone, rx, cmd_tx));
        });

        Some(Self { state, tx })
    }

    pub fn update(&self, player: &AudioPlayer) {
        {
            let mut s = self.state.lock().unwrap();
            if let Some(path) = &player.now_playing {
                s.stopped = false;
                s.paused = player.paused;
                s.title = player
                    .track_info
                    .as_ref()
                    .and_then(|t| t.title.clone())
                    .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                s.artist = player.track_info.as_ref().and_then(|t| t.artist.clone());
                s.album = player.track_info.as_ref().and_then(|t| t.album.clone());
                s.length_us = player.total_duration.map(|d| d.as_micros() as i64);
            } else {
                *s = State::default();
            }
        }
        let _ = self.tx.try_send(());
    }
}

async fn run(
    state: Arc<Mutex<State>>,
    mut rx: tokio::sync::mpsc::Receiver<()>,
    cmd_tx: StdSender<Cmd>,
) {
    let player = match Player::builder("tui_fi")
        .identity("tui-fi")
        .can_play(true)
        .can_pause(true)
        .can_go_next(true)
        .can_go_previous(true)
        .can_seek(false)
        .build()
        .await
    {
        Ok(p) => p,
        Err(_) => return,
    };

    {
        let tx = cmd_tx.clone();
        player.connect_play(move |_| { let _ = tx.send(Cmd::Play); });
        let tx = cmd_tx.clone();
        player.connect_pause(move |_| { let _ = tx.send(Cmd::Pause); });
        let tx = cmd_tx.clone();
        player.connect_play_pause(move |_| { let _ = tx.send(Cmd::Toggle); });
        let tx = cmd_tx.clone();
        player.connect_stop(move |_| { let _ = tx.send(Cmd::Stop); });
        let tx = cmd_tx.clone();
        player.connect_next(move |_| { let _ = tx.send(Cmd::Next); });
        let tx = cmd_tx;
        player.connect_previous(move |_| { let _ = tx.send(Cmd::Previous); });
    }

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            tokio::task::spawn_local(player.run());

            while rx.recv().await.is_some() {
                let s = state.lock().unwrap().clone();

                let playback = if s.stopped {
                    PlaybackStatus::Stopped
                } else if s.paused {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Playing
                };

                let mut meta = Metadata::builder();
                if !s.title.is_empty() {
                    meta = meta.title(s.title.clone());
                }
                if let Some(a) = &s.artist {
                    meta = meta.artist([a.clone()]);
                }
                if let Some(a) = &s.album {
                    meta = meta.album(a.clone());
                }
                if let Some(us) = s.length_us {
                    meta = meta.length(Time::from_micros(us));
                }

                let _ = player.set_playback_status(playback).await;
                let _ = player.set_metadata(meta.build()).await;
            }
        })
        .await;
}
