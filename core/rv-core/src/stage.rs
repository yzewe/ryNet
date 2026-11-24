use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;

tokio::task_local! {
    static STAGE: Rc<Cell<&'static str>>;
}

pub(crate) fn set(stage: &'static str) {
    let _ = STAGE.try_with(|c| c.set(stage));
}

pub(crate) fn scope<F: Future>(stage: Rc<Cell<&'static str>>, f: F) -> impl Future<Output = F::Output> {
    STAGE.scope(stage, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stage_is_seen_after_timeout_and_ignored_outside_scope() {
        set("tcp");
        let stage = Rc::new(Cell::new("connect"));
        let stuck = async {
            set("reality");
            std::future::pending::<()>().await
        };
        let r = tokio::time::timeout(std::time::Duration::from_millis(10), scope(stage.clone(), stuck)).await;
        assert!(r.is_err());
        assert_eq!(stage.get(), "reality");
    }
}
