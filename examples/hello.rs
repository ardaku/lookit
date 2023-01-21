use lookit::Searcher;
use pasts::prelude::*;

#[async_main::async_main]
async fn main(_spawner: impl Spawn) {
    let mut searcher = Searcher::with_camera();
    loop {
        let file = searcher
            .next()
            .await
            .file_open()
            .or_else(|it| it.file_open_r())
            .or_else(|it| it.file_open_w())
            .ok();
        dbg!(file);
    }
}
