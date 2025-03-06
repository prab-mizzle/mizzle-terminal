use std::{process::Stdio, sync::Arc};

use crate::obj::{BindingStatus, ContainerBindingResponse};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use tokio::{process::Command, sync::watch::Sender};

//parametrize the function to accept diff. confg. as per future requirement
pub async fn open_terminal(
    Path(instance_id): Path<String>,
    State((mut job_handle, session_handle)): State<(
        Arc<DashMap<String, (String, Sender<()>)>>,
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
    let session_path = format!("/tmp/{}.sock", uds_session_name);

    let args = vec![
        "-O",
        "-o",
        "-a",
        "-W",
        "-i",
        &session_path,
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

            // update hashmaps for proper information
            job_handle.insert(uds_session_name.clone(), (instance_id.clone(), tx));
            session_handle.insert(instance_id.clone(), uds_session_name.clone());

            let domain = format!(
                "{}/{}/{}",
                dotenv!("SERVING_NAME"),
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
    State((mut job_handle, session_handle)): State<(
        Arc<DashMap<String, (String, Sender<()>)>>,
        Arc<DashMap<String, String>>,
    )>,
) -> impl IntoResponse {
    let resp = match job_handle.contains_key(&session_id) {
        true => {
            if let Some((sess, handle)) = job_handle.remove(&session_id) {
                if let Ok(val) = handle.1.send(()) {
                    // cancel this running task
                    println!("+ dropped session: {sess}");
                    let instance_id = handle.0;
                    // once the task is dropped, remove the session from session_handle hashmap
                    session_handle.remove(&instance_id);
                    Response::new("+ successfully closed the terminal session".to_string())
                } else {
                    //session cancellation failed
                    println!("- failed to cancel the session: {sess}");
                    Response::new("- failed to cancel the session".to_string())
                }
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
