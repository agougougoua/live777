use std::collections::HashSet;

use tracing::{error, info};

use crate::config::CascadeMode;
use crate::route::utils::{cascade_pull, cascade_push, force_check_times, session_delete};
use crate::store::Server;
use crate::{result::Result, AppState};

pub async fn cascade_new_node(
    mut state: AppState,
    nodes: Vec<Server>,
    stream: String,
) -> Result<Server> {
    let set_all: HashSet<Server> = state.storage.nodes().await.into_iter().collect();
    let set_src: HashSet<Server> = nodes.clone().into_iter().collect();
    let set_dst: HashSet<&Server> = set_all.difference(&set_src).collect();
    let arr = set_dst.into_iter().collect::<Vec<&Server>>();

    let server_src = nodes.first().unwrap().clone();
    let server_ds0 = *arr.first().unwrap();
    let server_dst = server_ds0.clone();

    let mode = state.config.cascade.mode.clone();
    let public = state.config.http.public.clone();
    let client = state.client.clone();

    info!(
        "cascade mode: {:?}, from: {:?}, to: {:?}",
        mode, server_src, server_dst
    );

    tokio::spawn(async move {
        let cascade_result = match mode {
            CascadeMode::Push => {
                cascade_push(
                    public,
                    client.clone(),
                    server_src.clone(),
                    server_dst.clone(),
                    stream.clone(),
                )
                .await
            }
            CascadeMode::Pull => {
                cascade_pull(
                    state.client.clone(),
                    server_src.clone(),
                    server_dst.clone(),
                    stream.clone(),
                )
                .await
            }
        };
        match cascade_result {
            Ok(()) => {
                match force_check_times(
                    state.client.clone(),
                    server_dst.clone(),
                    stream.clone(),
                    state.config.cascade.check_attempts.0,
                )
                .await
                {
                    Ok(count) => {
                        if state.config.cascade.close_other_sub {
                            cascade_close_other_sub(state, server_src, stream).await
                        }
                        info!("cascade {:?} success, checked attempts: {}", mode, count)
                    }
                    Err(e) => error!("cascade check error: {:?}", e),
                }
            }
            Err(e) => error!("cascade {:?} error: {:?}", mode, e),
        }
    });

    Ok(server_ds0.clone())
}

async fn cascade_close_other_sub(mut state: AppState, server: Server, stream: String) {
    match state.storage.info_get(server.clone().alias).await {
        Ok(streams) => {
            for stream_info in streams.into_iter() {
                if stream_info.id == stream {
                    for sub_info in stream_info.subscribe.sessions.into_iter() {
                        match sub_info.cascade {
                            Some(v) => info!("Skip. Is cascade: {:?}", v),
                            None => {
                                match session_delete(
                                    state.client.clone(),
                                    server.clone(),
                                    stream.clone(),
                                    sub_info.id,
                                )
                                .await
                                {
                                    Ok(_) => {}
                                    Err(e) => error!("cascade close other sub error: {:?}", e),
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(e) => error!("cascade don't closed other sub: {:?}", e),
    }
}
