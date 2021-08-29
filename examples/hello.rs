use lookit::Lookit;

async fn run() {
    let mut lookit = Lookit::with_input().expect("no /dev/ access?");

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
    pasts::block_on(run());
}
