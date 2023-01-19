use warp::Filter;

#[tokio::main]
async fn main() {
    warp::serve(
            warp::path("ttt").and(
//                 warp::path(".git").and(warp::path::tail()).map(|tail|{ 
// let d = format!("resource or api {:?} is not defined", tail);
// warp::reply::json(&serde_json::json!({
//                 "errors": [
//                     { "detail": d }
//                 ]
//             }))})
                warp::path!(".git/aaa").and(warp::path::tail()).map(|tail| format!("{:?}", tail))
                .or(
                    warp::fs::dir("../index")
                   )
                )
           )
        .run(([127, 0, 0, 1], 3030))
        .await;
}

