#[macro_use]
extern crate dotenv_codegen;

use std::{sync::Arc};

use axum::{routing::get, Router};
use dashmap::DashMap;
use dotenv::var;
use tokio::sync::watch::Sender;
use utils::shutdown_signal;

mod obj;
mod routes;
mod utils;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let port = format!("0.0.0.0:{}", dotenv!("TERM_SERVER_PORT"));
    // let port = format!(
    //     "0.0.0.0:{}",
    //     var("SERVER_PORT").unwrap_or("9001".to_string())
    // );
    println!("+ Server runnning on address: {port}");

    // Insert checks for presence for bore & ttyd !
    let ttyd_util = "ttyd";
    let bore_util = "bore";

    let bore_version = b"bore-cli 0.5.2";
    let ttyd_version = b"ttyd version 1.7.4-68521f5";

    let ttyd_check = utils::get_program_version(&ttyd_util);
    let bore_check = utils::get_program_version(&bore_util);

    match tokio::join!(ttyd_check, bore_check) {
        (Ok(ttyd), Ok(bore)) if bore == bore_version && ttyd == ttyd_version => {
            println!("✔ ttyd present ");
            println!("✔ bore present ");
        }

        (Ok(ttyd), _) if ttyd != ttyd_version => {
            println!("𝘟 ttyd incorrect version present: {}", unsafe {
                std::str::from_utf8_unchecked(&ttyd)
            });
        }

        (_, Ok(bore)) if bore != bore_version => {
            println!("𝘟 bore incorrect version present ");
        }

        (Err(ttyd_err), Err(bore_err)) => {
            println!("- both program failed to run ! ttyd: {ttyd_err}, bore: {bore_err}");
            return; //exit program gracefully
        }

        (_, Err(bore_err)) => {
            println!("- bore program failed to run !: {bore_err}");
            return; //exit program gracefully
        }
        (Err(ttyd_err), _) => {
            println!("- ttyd program failed to run !: {ttyd_err}");
            return; //exit program gracefully
        }
        _ => {
            println!("- both program abruptly failed to run ");
            return;
        }
    };

    let listener = match tokio::net::TcpListener::bind(port).await {
        Ok(val) => {
            println!("+ port binding succeded");
            val
        }
        Err(err) => {
            println!("- port binding failed: {err}");
            return;
        }
    };

    // maps instances id  with session id
    let mut global_instance_state: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
    // maps session_id with instance task handle
    let mut global_handle_state: Arc<DashMap<String, Sender<()>>> = Arc::new(DashMap::new());

    let route_1 = Router::new()
        .route("/start_web_session/{uid}", get(routes::open_terminal))
        .with_state((global_handle_state.clone(), global_instance_state.clone()));
    let route_2 = Router::new()
        .route("/terminate_web_session/{uid}", get(routes::close_terminal))
        .with_state(global_handle_state);

    let app = Router::new().merge(route_1).merge(route_2);
    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()); 
    if let Err(err) = server.await {
        println!("- error starting the application {err}");
    }
}


