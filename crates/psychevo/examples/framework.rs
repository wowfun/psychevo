use psychevo::{Application, StartThreadRequest, ThreadListQuery, TurnEvent, TurnRequest};

#[tokio::main]
async fn main() -> psychevo::Result<()> {
    let application = Application::builder()
        .home("./.psychevo-sdk")
        .build()
        .await?;
    let client = application.client();

    let thread = client.start_thread(StartThreadRequest::new(".")).await?;
    let turn = thread
        .start_turn(TurnRequest::new("Find the main entrypoints"))
        .await?;

    let mut events = turn.events();
    while let Some(event) = events.next().await {
        if let TurnEvent::ReasoningDelta { text } = event {
            eprint!("{text}");
        }
    }

    let result = turn.wait().await?;
    println!("{}", result.final_answer);

    let snapshot = thread.snapshot().await?;
    assert_eq!(snapshot.id, thread.id());
    let _summaries = client.list_threads(ThreadListQuery::default()).await?;

    application.shutdown().await?.require_clean()?;
    Ok(())
}
