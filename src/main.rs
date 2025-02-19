use axum::{routing::get, Router};
use tokio::sync::{broadcast::{self, Receiver, Sender}, Notify}; 

mod routes;
mod utils; 
mod obj;

#[tokio::main]
async fn main() {

    let port = "0.0.0.0:9001"; 

    // Insert checks for presence for bore & ttyd ! 
    let ttyd_util = "ttyd";
    let bore_util = "bore";
    
    let bore_version = b"bore-cli 0.5.2";  
    let ttyd_version = b"ttyd version 1.7.4-68521f5"; 

    let ttyd_check = utils::get_program_version(&ttyd_util); 
    let bore_check = utils::get_program_version(&bore_util); 
    
    match tokio::join!(ttyd_check, bore_check) { 

        (Ok(ttyd), Ok(bore)) 
            if bore == bore_version && ttyd == ttyd_version => {
                println!("✔ ttyd present ");
                println!("✔ bore present ");
        }

        (Ok(ttyd), _)
            if ttyd != ttyd_version => {
                println!("𝘟 ttyd incorrect version present: {}", unsafe { std::str::from_utf8_unchecked(&ttyd) });
        }

        (_, Ok(bore)) 
            if bore != bore_version => {
                println!("𝘟 bore incorrect version present ");
        }

        (Err(ttyd_err), Err(bore_err)) => {
            println!("- both program failed to run ! ttyd: {ttyd_err}, bore: {bore_err}");
            return ; //exit program gracefully
        }
        
        (_, Err(bore_err)) => {
            println!("- bore program failed to run !: {bore_err}");
            return ; //exit program gracefully
        }
        (Err(ttyd_err), _) => {
            println!("- ttyd program failed to run !: {ttyd_err}");
            return ; //exit program gracefully
        }
        _ => { 
            println!("- both program abruptly failed to run ");
            return ; 
        }

    };

    let listener = match tokio::net::TcpListener::bind(port).await
    {
        Ok(val) => {
            println!("+ port binding succeded");
            val
        }
        Err(err) => {
            println!("- port binding failed: {err}");
            return ;
        }
    };

    let (mut sender_tx, mut waker_tx ) = broadcast::channel::<String>(16);
    let mut notifier = std::sync::Arc::new(Notify::new()); 
    let clonable_recv = ClonableRecv::new(sender_tx.clone()); 

    let route_1 = Router::new()
        .route("/start_web_session/{uid}", get(routes::open_terminal))
        .with_state((clonable_recv, notifier.clone()));
    let route_2 = Router::new()
        .route("/terminate_web_session/{uid}", get(routes::close_terminal))
        .with_state(sender_tx.clone()); 

    let app= Router::new().merge(route_1).merge(route_2);

    if let Err(err) = axum::serve(listener, app).await { 
        println!("- error starting the application {err}");
    }

    notifier.notify_waiters();

}   

struct ClonableRecv(pub Sender<String>,pub Receiver<String>);

impl ClonableRecv { 
    pub fn new(sender: Sender<String>) -> Self {
        let recv = sender.subscribe(); 
        ClonableRecv(sender.clone(), recv)
    }
}

impl std::clone::Clone for ClonableRecv {
    fn clone(&self) -> Self {
        let sender = self.0.clone(); 
        let recv = self.0.subscribe(); 
        ClonableRecv(sender, recv)
    }
}