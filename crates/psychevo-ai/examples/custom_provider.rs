use futures::{StreamExt, stream};
use psychevo_ai::{
    AdapterCall, AdapterFuture, AdapterStream, DeploymentConfig, GenerationEvent, LanguageAdapter,
    LanguageAdapterEvent, LanguageRequest, Message, Provider,
};

#[derive(Debug)]
struct LocalLanguageAdapter;

impl LanguageAdapter for LocalLanguageAdapter {
    fn stream(
        &self,
        call: AdapterCall<LanguageRequest>,
    ) -> AdapterFuture<'_, AdapterStream<LanguageAdapterEvent>> {
        let model = call.model;
        Box::pin(async move {
            Ok(Box::pin(stream::iter([
                Ok(LanguageAdapterEvent::TextStart { content_index: 0 }),
                Ok(LanguageAdapterEvent::TextDelta {
                    content_index: 0,
                    delta: format!("custom provider answered with {model}"),
                }),
                Ok(LanguageAdapterEvent::TextEnd { content_index: 0 }),
                Ok(LanguageAdapterEvent::Finish {
                    finish_reason: None,
                }),
            ])) as AdapterStream<_>)
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let provider = Provider::builder(DeploymentConfig::new("local", "example", "example://local"))
        .language_adapter(LocalLanguageAdapter)
        .build()?;
    let model = provider.language_model("org/model")?;
    let mut generation = model.stream(LanguageRequest {
        messages: vec![Message::user("hello")],
        ..LanguageRequest::default()
    });

    while let Some(event) = generation.next().await {
        if let GenerationEvent::TextDelta { delta, .. } = event? {
            print!("{delta}");
        }
    }
    println!();
    Ok(())
}
