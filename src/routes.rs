use std::{process::Stdio, sync::Arc};

use axum::{
    body::Body,
    extract::{ws::Message, Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::UnixStream,
    process::Command,
    sync::watch::Sender,
    task::JoinHandle,
};
// use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
use tokio_util::codec::{Framed, LinesCodec};

use crate::obj::{BindingStatus, ContainerBindingResponse};

//parametrize the function to accept diff. confg. as per future requirement
pub async fn open_terminal(
    Path(instance_id): Path<String>,
    State((mut job_handle, session_handle)): State<(
        Arc<DashMap<String, Sender<()>>>,
        Arc<DashMap<String, String>>,
    )>,
) -> impl IntoResponse {
    // apply checks for presence of container on system !

    let is_sanitized = instance_id.chars().all(|c| c.is_alphanumeric());
    let char_count = instance_id.chars().count();

    //todo: ensure max string size is correct
    if !is_sanitized || char_count < 6 || char_count > 20 {
        println!("- instance ID check failed");
        return Err(BindingStatus::Failed(
            "Incorrect instance id: {instance_id}".to_string(),
        ));
    }

    if let Some(session_id) = session_handle.get(&instance_id) {
        println!("+ session already found");
        return Err(BindingStatus::SessionRunning(session_id.clone()));
    }
    let uds_session_name = uuid::Uuid::new_v4().as_hyphenated().to_string();

    //todo: check if files which are created has 660 permission or not
    let sessin_path = format!("/tmp/{}.sock", uds_session_name);

    let args = vec![
        "-O",
        "-o",
        "-a",
        "-W",
        "-i",
        &uds_session_name,
        "lxc",
        "exec",
        &instance_id,
        "--",
        "sh",
        "-c",
        r#"/bin/bash"#,
    ];

    let ttyd_session = Command::new("ttyd")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();

    match ttyd_session {
        Ok(mut ttyd) => {
            println!("+ listening on domain socket: {uds_session_name}");

            let (mut tx, mut recv) = tokio::sync::watch::channel(());

            tokio::spawn(async move {
                let ttyd_pid = ttyd.id();
                tokio::select! {
                    _ = ttyd.wait() => {
                            // when bore finishes it must ensure that any associated ttyd task is also dropped

                        // bore.kill().await;
                    }

                    _ = recv.changed() =>  {
                        println!("- task shutdown req : ");

                        match ttyd.kill().await {
                            Ok(_) => {
                                println!("+ process shutdown success: pid ttyd: {ttyd_pid:?}");
                            }
                            _ => {
                                println!(" - process failed abruptly !");
                            }
                        }
                        ()
                    }
                }
                ()
            });

            job_handle.insert(uds_session_name.clone(), tx);
            session_handle.insert(instance_id.clone(), uds_session_name.clone());

            let domain = format!(
                "http://localhost:8080/{}/{}",
                dotenv!("MACHINE_ID"),
                uds_session_name
            );
            let container_binding_resp = ContainerBindingResponse {
                session_id: uds_session_name,
                url: domain, //todo: change this to something concrete
                status: BindingStatus::Live,
            };

            return Ok(container_binding_resp);
        }
        Err(ttyd_err) => {
            println!("- ttyd process failed to start: {ttyd_err}");
            return Err(BindingStatus::Failed(format!(
                "ttyd spawing error: {}",
                ttyd_err.to_string()
            )));
        }
    }
    unreachable!("unreachable code section")
}

pub async fn close_terminal(
    Path(session_id): Path<String>,
    State(job_handle): State<Arc<DashMap<String, Sender<()>>>>,
) -> impl IntoResponse {
    let resp = match job_handle.contains_key(&session_id) {
        true => {
            if let Some((sess, handle)) = job_handle.remove(&session_id) {
                handle.send(()); // cancel this running task
                println!("+ dropped session: {sess}");
                // todo: remove the session from session_handle hashmap
                Response::new("+ successfully closed the terminal session".to_string())
            } else {
                println!("- failed to close session id");
                Response::new("- job not found for session_key or already removed".to_string())
            }
        }
        false => {
            println!("- failed to find the task for a given session key");
            let mut resp = Response::new(format!("- task not found for session key"));
            *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            resp
        }
    };

    return resp;
}

pub async fn access_terminal(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    State(job_handle): State<Arc<DashMap<String, Sender<()>>>>,
) -> impl IntoResponse {
    if job_handle.contains_key(&session_id) {
        //we don't require sanitation code here, since we already have verified
        // the right session id is already verified to be present in the hashmap
        let recv = job_handle
            .get(&session_id)
            .expect("we have preverfied the presence of key in dashmap")
            .subscribe();

        ws.on_upgrade(move |socket| handle_websocket(socket, session_id, recv))
    } else {
        println!("- web socket session not found");
        let body = Body::new(format!("- task not found for session uid in hashmap"));
        let mut resp = Response::new(body);
        *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        resp
    }
}

// async fn websocket_handler(ws: WebSocketUpgrade, Path(session_id): Path<String>) -> impl IntoResponse {
//     ws.on_upgrade(move |socket| handle_websocket(socket, session_id))
// }

// WebSocket Proxy Handler
async fn handle_websocket(
    socket: axum::extract::ws::WebSocket,
    session_id: String,
    mut recv: tokio::sync::watch::Receiver<()>,
) {
    let unix_socket_path = format!("/tmp/{}.sock", session_id);

    match UnixStream::connect(&unix_socket_path).await {
        Ok(unix_stream) => {
            let (mut user_ws_sender, mut user_ws_receiver) = socket.split();
            let (mut tty_sender, mut tty_receiver) =
                Framed::new(unix_stream, LinesCodec::new()).split();
            // let (mut tty_sender , mut tty_recv) = Framed::new()

            let user_to_tty = async move {
                while let Some(Ok(msg)) = user_ws_receiver.next().await {
                    if let Message::Text(text) = msg {
                        let text_str = text.to_string();
                        let _ = tty_sender.send(text_str).await;
                    }
                }
            };

            let tty_to_user = async move {
                while let Some(Ok(line)) = tty_receiver.next().await {
                    let _ = user_ws_sender.send(Message::Text(line.into())).await;
                }
            };

            tokio::select! {
                _ = user_to_tty => {
                    println!("+ user_to_tty task completed");
                },
                _ = tty_to_user => {
                    println!("+ tty_to_user task completed");
                },
                _ = recv.changed() => {
                    println!("+ task shutdown req : ");
                    return;
                }
            }
        }
        Err(err) => {
            eprintln!("Failed to connect to ttyd socket: {}", err);
        }
    }
}
