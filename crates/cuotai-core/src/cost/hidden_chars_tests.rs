//! PRD 0042 re-validation S-05: labels and folder names never carry a
//! control, direction or other invisible character, in what is stored and
//! in what is read back from an index written before the filter.

use super::*;

const RLO: char = '\u{202E}';

#[test]
fn a_label_with_a_hidden_character_is_refused() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    three_sessions(&f);
    f.scan();
    let index = f.scanner.index();
    let project = index.projects().unwrap()[0].project.clone();
    let session = index.session_labels().unwrap();
    assert!(session.is_empty());
    for label in [
        format!("Client{RLO}X"),
        "Client\u{200B}work".to_owned(),
        "Client\u{2066}work".to_owned(),
        "Client\u{1b}]0;x\u{7}".to_owned(),
        "Client\u{E0041}".to_owned(),
    ] {
        assert_eq!(
            index.set_label(LabelTarget::Project, &project, Some(&label)),
            Err(CostError::LabelCharacters),
            "{label:?}"
        );
    }
    assert_eq!(
        CostError::LabelCharacters.l10n().id,
        "cost.error.label_characters"
    );
    // An accent or a script other than Latin is visible text.
    index
        .set_label(LabelTarget::Project, &project, Some("Clienté 客户"))
        .unwrap();
    assert!(index
        .projects()
        .unwrap()
        .iter()
        .any(|p| p.label.as_deref() == Some("Clienté 客户")));

    // A backup's label with such a character is skipped like an invalid one.
    let labels = Labels {
        schema: 1,
        key: None,
        projects: vec![ProjectLabel {
            project: index.projects().unwrap()[1].project.clone(),
            label: Some(format!("Imported{RLO}")),
            hidden: false,
        }],
        sessions: Vec::new(),
    };
    assert_eq!(index.import_labels(&labels, true).unwrap(), 0);
}

#[test]
fn a_folder_name_drops_hidden_characters() {
    assert_eq!(
        source::folder_name(&format!("/srv/secret-proj{RLO}evil\u{200B}")).as_deref(),
        Some("secret-projevil")
    );
    assert_eq!(source::folder_name(&format!("/srv/{RLO}\u{2066}")), None);
}

#[test]
fn labels_and_folder_names_stored_before_the_filter_read_back_clean() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    three_sessions(&f);
    f.scan();
    let pricing = pricing(&f);
    let span = Span::Period(Period::Today);
    let index = f.scanner.index();
    let page = f.scanner.sessions(&pricing, span, now(), None, 0).unwrap();
    let session = page.sessions[0].session.clone();
    // Planted as an older version could have stored them.
    let db = rusqlite::Connection::open(index.path()).unwrap();
    db.execute("UPDATE projects SET label = ?1", [format!("Client{RLO}X")])
        .unwrap();
    db.execute(
        "INSERT OR REPLACE INTO project_hints (project, source, folder_hint)
         SELECT project, ?1, ?2 FROM projects",
        [SourceId::ClaudeCode.slug().to_owned(), format!("secret{RLO}evil")],
    )
    .unwrap();
    db.execute(
        "INSERT INTO session_labels (session, label) VALUES (?1, ?2)",
        [session.clone(), format!("Refactor{RLO}\u{200B}")],
    )
    .unwrap();
    drop(db);

    for row in index.projects().unwrap() {
        assert_eq!(row.label.as_deref(), Some("ClientX"));
        assert_eq!(row.folder_hint.as_deref(), Some("secretevil"));
    }
    assert_eq!(index.session_labels().unwrap()[&session], "Refactor");
    let summary = f.scanner.summary(&pricing, None, None, now()).unwrap();
    let projects = f.scanner.projects(&pricing, span, now()).unwrap();
    let sessions = f.scanner.sessions(&pricing, span, now(), None, 0).unwrap();
    let outputs = [
        serde_json::to_string(&summary).unwrap(),
        serde_json::to_string(&projects).unwrap(),
        serde_json::to_string(&sessions).unwrap(),
        serde_json::to_string(&index.labels().unwrap()).unwrap(),
    ];
    for json in &outputs {
        assert!(!json.contains(RLO) && !json.contains('\u{200B}'), "{json}");
    }
    assert!(outputs[0].contains("ClientX"), "{}", outputs[0]);
    assert!(outputs[1].contains("ClientX"), "{}", outputs[1]);
    assert!(outputs[2].contains("Refactor"), "{}", outputs[2]);
    assert!(outputs[3].contains("ClientX") && outputs[3].contains("Refactor"));

    // A stored value with nothing visible left reads as no label.
    let db = rusqlite::Connection::open(index.path()).unwrap();
    db.execute("UPDATE projects SET label = ?1", [format!("{RLO}\u{200B}")])
        .unwrap();
    drop(db);
    assert!(index.projects().unwrap().iter().all(|p| p.label.is_none()));
}
