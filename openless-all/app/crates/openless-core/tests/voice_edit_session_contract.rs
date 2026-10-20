use openless_core::{
    EditOperation, EditPlan, TextSelection, VoiceEditError, VoiceEditPhase, VoiceEditSession,
    VoiceEditTarget,
};

fn literal(find: &str, replace: &str) -> EditPlan {
    EditPlan {
        operations: vec![EditOperation::LiteralReplace {
            find: find.into(),
            replace: replace.into(),
        }],
        summary: Some(format!("replace {find}")),
    }
}

#[test]
fn draft_uses_existing_field_and_appends_the_first_dictated_segment() {
    let mut session = VoiceEditSession::start("已有内容", None).unwrap();
    session.finish_dictation("，这是口述补充").unwrap();

    let snapshot = session.snapshot();
    let context = snapshot.context.as_ref().unwrap();
    assert_eq!(context.field_text, "已有内容");
    assert_eq!(context.dictated_segment, "，这是口述补充");
    assert_eq!(context.draft, "已有内容，这是口述补充");
    assert_eq!(context.preview, context.draft);
    assert_eq!(snapshot.phase, VoiceEditPhase::DraftReady);
    assert_eq!(snapshot.target, VoiceEditTarget::FullField);
}

#[test]
fn selected_context_is_the_edit_draft_but_keeps_the_full_field_attachment() {
    let selection = TextSelection { start: 2, end: 4 };
    let mut session = VoiceEditSession::start("甲乙丙丁", Some(selection)).unwrap();
    session.finish_dictation("口述不会覆盖选区上下文").unwrap();

    let context = session.snapshot().context.unwrap();
    assert_eq!(context.field_text, "甲乙丙丁");
    assert_eq!(context.selection, Some(selection));
    assert_eq!(context.draft, "丙丁");
    assert_eq!(
        session.snapshot().target,
        VoiceEditTarget::Selection { start: 2, end: 4 }
    );
}

#[test]
fn multiple_instruction_turns_are_applied_to_preview_and_commit_only_happens_once() {
    let mut session = VoiceEditSession::start("今天下雨", None).unwrap();
    session.finish_dictation("").unwrap();

    session.enter_editing().unwrap();
    session.begin_applying().unwrap();
    session
        .apply_instruction("改成晴天", "改成晴天", literal("下雨", "晴天"))
        .unwrap();
    session.enter_editing().unwrap();
    session.begin_applying().unwrap();
    session
        .apply_instruction("再正式一点", "再正式一点", literal("今天", "今日"))
        .unwrap();

    let snapshot = session.snapshot();
    assert_eq!(snapshot.phase, VoiceEditPhase::Preview);
    assert_eq!(snapshot.context.as_ref().unwrap().preview, "今日晴天");
    assert_eq!(snapshot.context.as_ref().unwrap().turns.len(), 2);
    assert_eq!(
        snapshot.context.as_ref().unwrap().turns[0].preview_after,
        "今天晴天"
    );

    let commit = session.begin_commit().unwrap();
    assert_eq!(commit.text, "今日晴天");
    assert_eq!(session.snapshot().phase, VoiceEditPhase::Committing);
    assert!(matches!(
        session.begin_commit(),
        Err(VoiceEditError::InvalidPhase { .. })
    ));
    assert!(matches!(
        session.cancel(),
        Err(VoiceEditError::InvalidPhase { .. })
    ));
    session.complete_commit().unwrap();
    assert_eq!(session.snapshot().phase, VoiceEditPhase::Completed);
    assert!(matches!(
        session.begin_commit(),
        Err(VoiceEditError::InvalidPhase { .. })
    ));
}

#[test]
fn instruction_errors_restore_a_retryable_draft_or_preview() {
    for edited in [false, true] {
        for applying in [false, true] {
            let mut session = VoiceEditSession::start("草稿", None).unwrap();
            session.finish_dictation("").unwrap();
            if edited {
                session
                    .apply_instruction("改成预览", "改成预览", literal("草稿", "预览"))
                    .unwrap();
            }
            let before = session.snapshot();
            session.enter_editing().unwrap();
            if applying {
                session.begin_applying().unwrap();
                assert_eq!(session.snapshot().phase, VoiceEditPhase::Applying);
            }
            session.recover_instruction().unwrap();
            assert_eq!(session.snapshot(), before);
            session.enter_editing().unwrap();
        }
    }
}

#[test]
fn commits_are_rejected_before_the_draft_is_ready() {
    let mut session = VoiceEditSession::start("草稿", None).unwrap();
    assert!(session.begin_commit().is_err());
    assert!(session.complete_commit().is_err());
    assert!(session.recover_commit().is_err());
    assert_eq!(session.snapshot().phase, VoiceEditPhase::Dictating);

    session.finish_dictation("").unwrap();
    session.enter_editing().unwrap();
    assert!(session.begin_commit().is_err());
    assert_eq!(session.snapshot().phase, VoiceEditPhase::Editing);
    session.begin_applying().unwrap();
    assert!(session.begin_commit().is_err());
    assert_eq!(session.snapshot().phase, VoiceEditPhase::Applying);
}

#[test]
fn failed_target_writes_restore_the_previous_draft_and_allow_retry() {
    for edited in [false, true] {
        let mut session = VoiceEditSession::start("草稿", None).unwrap();
        session.finish_dictation("").unwrap();
        if edited {
            session
                .apply_instruction("改成预览", "改成预览", literal("草稿", "预览"))
                .unwrap();
        }
        let before = session.snapshot();
        let first_commit = session.begin_commit().unwrap();
        session.recover_commit().unwrap();
        assert_eq!(session.snapshot(), before);
        assert_eq!(session.begin_commit().unwrap(), first_commit);
        session.complete_commit().unwrap();
        assert_eq!(session.snapshot().phase, VoiceEditPhase::Completed);
        assert!(session.recover_commit().is_err());
        assert!(session.complete_commit().is_err());
    }
}

#[test]
fn cancellation_clears_the_pending_commit_and_never_returns_text() {
    let mut session = VoiceEditSession::start("草稿", None).unwrap();
    session.finish_dictation("").unwrap();
    session.cancel().unwrap();

    assert_eq!(session.snapshot().phase, VoiceEditPhase::Cancelled);
    assert!(session.snapshot().context.is_none());
    assert!(matches!(
        session.begin_commit(),
        Err(VoiceEditError::InvalidPhase { .. })
    ));
}
