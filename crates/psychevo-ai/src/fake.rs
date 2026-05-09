#[derive(Clone)]
pub struct FakeProvider {
    scripts: Arc<Mutex<VecDeque<Vec<RawStreamEvent>>>>,
}

impl FakeProvider {
    pub fn new(scripts: Vec<Vec<RawStreamEvent>>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into())),
        }
    }
}

impl GenerationProvider for FakeProvider {
    fn stream(
        &self,
        _request: GenerationRequest,
        abort: AbortSignal,
    ) -> BoxFuture<'static, Result<GenerationStream>> {
        let scripts = Arc::clone(&self.scripts);
        Box::pin(async move {
            if abort.aborted() {
                let events = vec![Ok(StreamEvent::Done {
                    outcome: Outcome::Aborted,
                    finish_reason: Some("aborted".to_string()),
                })];
                return Ok(Box::pin(stream::iter(events)) as Pin<Box<_>>);
            }

            let script = scripts
                .lock()
                .expect("fake provider script lock poisoned")
                .pop_front()
                .ok_or(Error::ScriptExhausted)?;
            let events = script.into_iter().map(|event| Ok(event.normalize()));
            Ok(Box::pin(stream::iter(events)) as Pin<Box<_>>)
        })
    }
}

