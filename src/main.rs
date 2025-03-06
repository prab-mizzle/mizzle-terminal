#[macro_use]
extern crate dotenv_codegen;

use std::sync::Arc;

use axum::{routing::get, Router};
use dashmap::DashMap;
use dotenv::var;
use tokio::sync::watch::Sender;
use utils::shutdown_signal;

mod obj;
mod routes;
mod utils;

// runtime variable settings

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let port = format!(
        "{}:{}",
        dotenv!("SERVER_INTERFACE_ADDR"),
        dotenv!("TERM_SERVER_PORT")
    );
    println!("+ Server runnning on address: {port}");

    // Insert checks for presence for bore & ttyd !
    let ttyd_util = "ttyd";

    let ttyd_version = b"ttyd version 1.7.4-68521f5";

    let ttyd_check = utils::get_program_version(&ttyd_util).await;

    match ttyd_check {
        Ok(ttyd) if ttyd == ttyd_version => {
            println!("✔ ttyd present ");
        }
        Err(ttyd_err) => {
            println!("- ttyd program failed to run !: {ttyd_err}");
            // return; //exit program gracefully
        }
        _ => {
            println!("- ttyd version mismatch, program failed abruptly !");
            // return;
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
    let global_instance_state: Arc<DashMap<String, String>> = Arc::new(DashMap::new());

    // maps session_id with instance task handle
    // key => session_id, value => (instance_id, Sender)
    let global_handle_state: Arc<DashMap<String, (String, Sender<()>)>> = Arc::new(DashMap::new());

    let route_1 = Router::new()
        .route(
            "/start_web_session/{instance_uid}",
            get(routes::open_terminal),
        )
        .with_state((global_handle_state.clone(), global_instance_state.clone()));
    let route_2 = Router::new()
        .route("/terminate_web_session/{uid}", get(routes::close_terminal))
        .with_state((global_handle_state.clone(), global_instance_state.clone()));

    let app = Router::new().merge(route_1).merge(route_2);

    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());

    if let Err(err) = server.await {
        println!("- error starting the application {err}");
    }
}
