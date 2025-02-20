use std::{process::Stdio, sync::Arc};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use dashmap::DashMap;
use random_port::PortPicker;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch::Sender,
    task::JoinHandle,
};

use crate::obj::{BindingStatus, ContainerBindingResponse};

#[axum::debug_handler]
//parametrize the function to accept diff. confg. as per future requirement
pub async fn open_terminal(
    Path(instance_id): Path<String>,
    State((mut job_handle, session_handle)): State<(
        Arc<DashMap<String, Sender<()>>>,
        Arc<DashMap<String, String>>,
    )>,
) -> impl IntoResponse {
    // apply checks for presence of container on system !

    //todo: implement sanitation
    let sanitized_instance_id = instance_id.trim().to_string();

    if let Some(session_id) = session_handle.get(&sanitized_instance_id) {
        println!("+ session already found");
        return Err(BindingStatus::SessionRunning(session_id.clone()));
    }

    // select port range for starting ttyd instance
    let random_port = match PortPicker::new().pick() {
        Ok(val) => {
            println!("+ Port binding succeed on: {val}");
            format!("{val}")
        }
        Err(err) => {
            println!("- Port binding failed");
            return Err(BindingStatus::PortAllocFailed(err.to_string()));
        }
    };

    let ttyd_session = Command::new("sudo")
        .arg("-n") // avoid password input for process
        .arg("ttyd")
        .arg("-O")
        .arg("-a")
        .arg("-W")
        .arg("-p")
        .arg(&random_port)
        .arg("lxc")
        .arg("exec")
        .arg(sanitized_instance_id)
        .arg("--")
        .arg("sh")
        .arg("-c")
        .arg(r#"/bin/bash"#)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();

    let bore_session = Command::new("bore")
        .arg("local")
        .arg(&random_port)
        .arg("--to")
        .arg("bore.pub")
        .stderr(Stdio::piped()) //change this our hosted instance
        .stdout(Stdio::piped()) //change this our hosted instance
        .kill_on_drop(true)
        .spawn();

    match (ttyd_session, bore_session) {
        (Ok(mut ttyd), Ok(mut bore)) => {
            //we would ignore logging ttyd for now !
            if let Some(bore_stdout) = bore.stdout.take() {
                let mut bore_stdout = BufReader::new(bore_stdout).lines();
                let first_line = bore_stdout.next_line().await;

                if let Ok(Some(log)) = first_line {
                    // Split the string by whitespace and get the last part
                    println!("port line: {log}");

                    //acquire port value which bore has been binded with !
                    let port_str = if let Some(last_part) = log.split_whitespace().last() {
                        // Extract the number after `remote_port=`
                        if let Some(number) = last_part.split('=').last() {
                            println!("+ Listening port: {}", number);
                            // number.parse::<u16>().expect("port number can't be converted to u16")
                            number
                        } else {
                            bore.kill().await;
                            ttyd.kill().await;
                            return Err(BindingStatus::PortNotFound(
                                "Failed to acquire port from logs".to_string(),
                            ));
                        }
                    } else {
                        bore.kill().await;
                        ttyd.kill().await;
                        return Err(BindingStatus::PortNotFound(
                            "Port value not found in logs !".to_string(),
                        ));
                    };

                    let domain = format!("http://bore.pub:{}", port_str);
                    println!("+ listening on domain : {domain}");
                    let session_uid = uuid::Uuid::new_v4().as_hyphenated().to_string();

                    let (mut tx, mut recv) = tokio::sync::watch::channel(());

                    tokio::spawn(async move {
                        // loop {
                        tokio::select! {
                            _ = ttyd.wait() => {
                                 // when bore finishes it must ensure that any associated ttyd task is also dropped
                                //  tokio::join!(ttyd.kill(), bore.kill());
                                // bore.kill().await;
                            }
                            _ = bore.wait() => {
                                // when bore finishes it must ensure that any associated ttyd task is also dropped
                                // tokio::join!(ttyd.kill(), bore.kill());
                                // ttyd.kill().await;
                            }
                            _ = recv.changed() =>  {
                                println!("- task shutdown req : ");
                                let pid = ttyd.id();
                                match (bore.kill().await, ttyd.kill().await) {
                                    (Ok(_), Ok(_)) => {
                                        println!("+ process shutdown success: pid ttyd: {pid:?}");
                                    }
                                    _ => {
                                        println!(" - process failed abruptly !");
                                    }
                                }
                                ()
                            }
                        }
                        // }
                        ()
                    });

                    job_handle.insert(session_uid.clone(), tx);

                    let container_binding_resp = ContainerBindingResponse {
                        session_id: Some(session_uid),
                        url: Some(domain),
                        status: BindingStatus::Live,
                    };

                    return Ok(container_binding_resp);
                } else {
                    println!("- Port reading error");
                    bore.kill().await;
                    ttyd.kill().await;
                    return Err(BindingStatus::ProcessReadError(
                        "error reading process line".to_string(),
                    ));
                }
            }
        }
        _ => {
            println!("process failed to start");
            let resp = ContainerBindingResponse {
                session_id: None,
                url: None,
                status: BindingStatus::Failed("error in binding container value".to_string()),
            };
            return Ok(resp);
        }
    }
    unreachable!("unreachable code section")
}

#[axum::debug_handler]
pub async fn close_terminal(
    Path(session_id): Path<String>,
    State(job_handle): State<Arc<DashMap<String, Sender<()>>>>,
) -> impl IntoResponse {
    let resp = match job_handle.contains_key(&session_id) {
        true => {
            if let Some((sess, handle)) = job_handle.remove(&session_id) {
                handle.send(()); // cancel this running task
                println!("+ dropped session: {sess}");
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
