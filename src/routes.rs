use std::{process::Stdio, str::FromStr, sync::Arc};

use axum::{extract::{Path, State}, http::StatusCode, response::{IntoResponse, Response}};
use random_port::PortPicker;
use tokio::{io::{AsyncBufReadExt, BufReader}, process::Command, sync::{broadcast::Receiver, broadcast::Sender, Notify}};
use uuid::Uuid;

use crate::{obj::{BindingStatus, ContainerBindingResponse}, ClonableRecv};

#[axum::debug_handler]
//parametrize the function to accept diff. confg. as per future requirement
pub async fn open_terminal(
        Path(instance_id): Path<String>, 
        State((mut shutdown_recv, global_shutdown_notifier)): State<(ClonableRecv, Arc<Notify>)>
) -> impl IntoResponse {
    
    //todo: implement sanitation 
    let sanitized_instance_id = instance_id.trim().to_string(); 

    // select port range for starting ttyd instance 
    let random_port = match PortPicker::new().pick() {
        Ok(val) =>  { 
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
        .spawn();

    let bore_session = Command::new("bore")
        .arg("local")
        .arg(&random_port)
        .arg("--to")
        .arg("bore.pub")
        .stderr(Stdio::piped()) //change this our hosted instance
        .stdout(Stdio::piped()) //change this our hosted instance
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
                        }
                        else { 
                            bore.kill().await; 
                            ttyd.kill().await; 
                            return Err(BindingStatus::PortNotFound("Failed to acquire port from logs".to_string()));
                        }
                    }
                    else { 
                        bore.kill().await; 
                        ttyd.kill().await; 
                        return Err(BindingStatus::PortNotFound("Port value not found in logs !".to_string()));
                    };

                    //set session values for repsonse from request !
                    let domain = match url::Url::from_str(&format!("http:bore.pub:{}", port_str)) {
                        Ok(val) => {
                            val
                        }
                        Err(err) => { 
                            println!("failed to parse url string: {}", err);
                            return Err(BindingStatus::Error(format!("Url parsing error: {}", err)));
                        }
                    };

                    let session_uid = uuid::Uuid::new_v4().as_hyphenated().to_string(); 

                    let shared_session_uid = session_uid.clone(); 

                    tokio::spawn(async move {  
                        let global_shutdown_handle = global_shutdown_notifier.notified(); 
                        tokio::pin!(global_shutdown_handle);
                        loop { 
                            tokio::select! { 
                                _ = ttyd.wait() => { 

                                }

                                _ = bore.wait() => { 

                                }

                                _ = &mut global_shutdown_handle => {
                                    println!("+ global shutdown req "); 
                                    bore.kill().await; 
                                    ttyd.kill().await; 
                                }

                                Ok(uid) = shutdown_recv.1.recv() =>  {
                                    if shared_session_uid == uid  {
                                        println!("+ shutdown req session uid: {}", uid); 
                                        bore.kill().await; 
                                        ttyd.kill().await; 
                                    }
                                }

                            }
                        }
                    });

                    let container_binding_resp = ContainerBindingResponse {  
                        session_id: Some(session_uid), 
                        url: Some(domain),
                        status: BindingStatus::Live, 
                    }; 

                    return Ok(container_binding_resp); 
                }
                else { 
                    println!("- Port reading error");
                    bore.kill().await; 
                    ttyd.kill().await; 
                    return Err(BindingStatus::ProcessReadError("error reading process line".to_string()));
                }
            }
        }
        _ => {
            println!("process failed to start");
            let resp = ContainerBindingResponse {
                session_id: None, 
                url: None, 
                status: BindingStatus::Failed("error in binding container value".to_string())
            };
            return Ok(resp); 
        }
    }
    unreachable!("unreachable code section")
}

#[axum::debug_handler]
pub async fn close_terminal(
    Path(session_id): Path<String>,
    State(shutdown_signal): State<Sender<String>>
) -> impl IntoResponse {

    let resp = match shutdown_signal.send(session_id.to_string()) {
        Ok(_) => { 
            Response::new("+ shutdown signal sent succesfully".to_string())
        }
        Err(err) => { 
            let mut resp = Response::new(format!("- unable to send shutdown signal: {}", err.to_string()));
            *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR; 
            resp
        }
    };

    return resp; 

}