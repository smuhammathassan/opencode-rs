use oc_command::question::{
    Answer, Info, QuestionError, QuestionId, QuestionOption, QuestionService, Tool,
};

fn info(question: &str) -> Info {
    Info {
        question: question.to_string(),
        header: question.to_string(),
        options: vec![QuestionOption {
            label: "yes".to_string(),
            description: "y".to_string(),
        }],
        multiple: None,
        custom: None,
    }
}

#[tokio::test]
async fn ask_registers_a_pending_request() {
    let svc = QuestionService::new();
    let (handle, request) = svc.ask("sess-1", vec![info("continue?")], None);
    assert!(request.id.to_string().starts_with("que_"));
    assert_eq!(request.session_id, "sess-1");
    assert_eq!(request.questions.len(), 1);
    assert!(svc.list().iter().any(|r| r.id == request.id));
    drop(handle);
}

#[tokio::test]
async fn reply_resolves_ask_with_answers() {
    let svc = QuestionService::new();
    let (handle, request) = svc.ask("sess-1", vec![info("continue?")], None);
    svc.reply(&request.id, vec![vec!["yes".to_string()]])
        .unwrap();
    let answers: Vec<Answer> = handle.await.unwrap();
    assert_eq!(answers, vec![vec!["yes".to_string()]]);
    assert!(!svc.list().iter().any(|r| r.id == request.id));
}

#[tokio::test]
async fn reject_resolves_ask_with_rejected_error() {
    let svc = QuestionService::new();
    let (handle, request) = svc.ask("sess-1", vec![info("continue?")], None);
    svc.reject(&request.id).unwrap();
    let err = handle.await.unwrap_err();
    assert!(matches!(err, QuestionError::Rejected));
    assert!(svc.list().is_empty());
}

#[tokio::test]
async fn reply_for_unknown_request_is_not_found() {
    let svc = QuestionService::new();
    let id = QuestionId::ascending();
    let err = svc.reply(&id, vec![vec!["x".to_string()]]).unwrap_err();
    assert!(matches!(err, QuestionError::NotFound { .. }));
}

#[tokio::test]
async fn reject_for_unknown_request_is_not_found() {
    let svc = QuestionService::new();
    let id = QuestionId::ascending();
    let err = svc.reject(&id).unwrap_err();
    assert!(matches!(err, QuestionError::NotFound { .. }));
}

#[tokio::test]
async fn list_returns_pending_requests() {
    let svc = QuestionService::new();
    let (handle, request) = svc.ask("sess-1", vec![info("a?"), info("b?")], None);
    let list = svc.list();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].questions.len(), 2);
    assert_eq!(list[0].session_id, "sess-1");
    drop(handle);
}

#[tokio::test]
async fn multiple_asks_can_be_resolved_independently() {
    let svc = QuestionService::new();
    let (handle1, request1) = svc.ask("sess-1", vec![info("a?")], None);
    let (handle2, request2) = svc.ask("sess-1", vec![info("b?")], None);
    assert_eq!(svc.list().len(), 2);
    assert_ne!(request1.id, request2.id);

    svc.reply(&request2.id, vec![vec!["two".to_string()]])
        .unwrap();
    assert_eq!(handle2.await.unwrap(), vec![vec!["two".to_string()]]);

    svc.reject(&request1.id).unwrap();
    assert!(handle1.await.is_err());
    assert!(svc.list().is_empty());
}

#[tokio::test]
async fn ids_are_unique_and_well_formed() {
    let a = QuestionId::ascending();
    let b = QuestionId::ascending();
    assert_ne!(a, b);
    assert_eq!(a.to_string().len(), "que_".len() + 26);
    assert!(a.to_string().starts_with("que_"));
}

#[tokio::test]
async fn tool_is_preserved_in_request() {
    let svc = QuestionService::new();
    let (handle, request) = svc.ask(
        "sess-1",
        vec![info("continue?")],
        Some(Tool {
            message_id: "msg-1".to_string(),
            call_id: "call-1".to_string(),
        }),
    );
    let tool = request.tool.unwrap();
    assert_eq!(tool.message_id, "msg-1");
    assert_eq!(tool.call_id, "call-1");
    drop(handle);
}

#[tokio::test]
async fn rejected_error_message_matches_reference() {
    assert_eq!(
        QuestionError::Rejected.to_string(),
        "The user dismissed this question"
    );
}
