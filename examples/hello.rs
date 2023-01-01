use lookit::Lookit;
use pasts::prelude::*;

async fn run() {
    let mut lookit = Lookit::with_camera();
    loop {
        let file = (&mut lookit)
            .await
            .file_open()
            .or_else(|it| it.file_open_r())
            .or_else(|it| it.file_open_w())
            .ok();
        dbg!(file);
    }
}

fn main() {
    Executor::default().spawn(run());
}
