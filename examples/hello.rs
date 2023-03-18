use lookit::Searcher;
use pasts::prelude::*;

#[async_main::async_main]
async fn main(_spawner: impl async_main::Spawn) {
    let mut searcher = Searcher::with_camera();
    loop {
        let file = searcher.next().await;

        dbg!(&file);

        let file = file
            .connect()
            .or_else(|it| it.connect_input())
            .or_else(|it| it.connect_output())
            .ok();

        dbg!(file);
    }
}
