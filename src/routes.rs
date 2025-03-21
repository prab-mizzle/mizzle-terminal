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

//parametrize the function to accept diff. confg. as per future requirement
pub async fn open_terminal(
    Path(instance_id): Path<String>,
    State((mut job_handle, session_handle)): State<(
        Arc<DashMap<String, (String, Sender<()>, tokio::time::Instant)>>,
        Arc<DashMap<String, String>>,
    )>,
) -> impl IntoResponse {
    // apply checks for presence of container on system !

    let is_sanitized = instance_id.chars().all(|c| c.is_alphanumeric());
    let char_count = instance_id.chars().count();
    
    //todo: ensure max string size is correct 
    if !is_sanitized ||  char_count < 6 || char_count > 20  {
        println!("- instance ID check failed");
        return Err(BindingStatus::Failed("Incorrect instance id: {instance_id}".to_string()));
    }

    if let Some(session_id) = session_handle.get(&instance_id) {
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

    let ttyd_session = Command::new("ttyd")
        .arg("-O")
        .arg("-o")
        .arg("-a")
        .arg("-W")
        .arg("-p")
        .arg(&random_port)
        // .arg("bash")
        .arg("lxc")
        .arg("exec")
        .arg(instance_id)
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
        // .arg("103.168.173.251:7835")
        // .arg("mizzleterminal.mizzle.io")
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

                            // Split the string at 'm' and take the part after it.
                            if let Some(digits) = number.rsplit('m').next() {
                                println!("Extracted digits: {}", digits);
                                digits.to_string()
                            } else {
                                println!("- No digits found in port !");
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
                    println!("port value: {:?}", port_str);
                    let domain = format!(r#"http://bore.pub:{}"#, port_str);
                    println!("+ listening on domain : {domain}");
                    let session_uid = uuid::Uuid::new_v4().as_hyphenated().to_string();

                    let (mut tx, mut recv) = tokio::sync::watch::channel(());

                    tokio::spawn(async move {
                        let ttyd_pid = ttyd.id();
                        tokio::select! {
                            _ = ttyd.wait() => {
                                 // when bore finishes it must ensure that any associated ttyd task is also dropped
                                //  tokio::join!(ttyd.kill(), bore.kill());
                                // bore.kill().await;
                                bore.kill().await;
                            }
                            _ = bore.wait() => {
                                // when bore finishes it must ensure that any associated ttyd task is also dropped
                                ttyd.kill().await;
                            }
                            _ = recv.changed() =>  {
                                println!("- task shutdown req : ");

                                match (bore.kill().await, ttyd.kill().await) {
                                    (Ok(_), Ok(_)) => {
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
            job_handle.insert(uds_session_name.clone(), (instance_id.clone(), tx, tokio::time::Instant::now()));
            session_handle.insert(instance_id.clone(), uds_session_name.clone());

            let mut domain = dotenv!("SERVING_NAME").to_string();
            let mut main_path = dotenv!("MACHINE_ID").to_string();
            if !main_path.is_empty() {
                domain.push_str("/");
            }
            main_path.push_str(format!("/{}", uds_session_name).as_str());
            domain.push_str(main_path.as_str());

            let jwt_secret = dotenv!("MZ_TERM_JWT_SINGING_SECRET"); 
            println!("+ jwt secret: {jwt_secret}");
            
            let jwt_timeout = dotenv!("MZ_TERM_JWT_TIMEOUT").parse::<u64>().unwrap_or(3600);
            
            match crate::utils::jwt_token_generator(jwt_timeout, jwt_secret) {
                Ok(token) => {
                    
                    //add token header as part of domain!
                    domain.push_str(format!("?token={}", token).as_str());

                    let container_binding_resp = ContainerBindingResponse {
                        terminal_session_name: uds_session_name,
                        access_token: token,
                        url: domain,
                        status: BindingStatus::Live,
                    };

                    return Ok(container_binding_resp);
                }
                Err(_) => {
                    return Err(BindingStatus::Error("JWT token generation failed".to_string()));
                }
            }



        }
        Err(ttyd_err) => {
            println!("- ttyd process failed to start: {ttyd_err}");
            return Err(BindingStatus::Failed(format!(
                "ttyd spawing error: {}",
                ttyd_err.to_string()
            )));
        }
        _ => {
            println!("process failed to start");
            return Err(BindingStatus::Failed(
                "error in binding container value".to_string(),
            ));
        }
    }
    unreachable!("unreachable code section")
}

pub async fn close_terminal(
    Path(session_id): Path<String>,
    State((mut job_handle, session_handle)): State<(
        Arc<DashMap<String, (String, Sender<()>, tokio::time::Instant)>>,
        Arc<DashMap<String, String>>,
    )>,
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
