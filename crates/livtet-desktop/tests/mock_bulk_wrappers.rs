//! Mock-app test for delete_editions wrapper — seeds and deletes.
use tauri::Manager;
use livtet_core::data::{Kind, TestDb};
use livtet_core::data::entities::{editions, works};
use livtet_core::data::orm::{ActiveModelTrait, Set};
use livtet_types::{DbId, now_primitive};
use livtet_desktop_lib::commands::bulk::delete_editions;
use livtet_desktop_lib::{AppState, mock_state_for_bulk};

#[tokio::test]
async fn delete_editions_wrapper_deletes_seeded_edition() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();

    let work_id = DbId::new();
    works::ActiveModel {
        id: Set(work_id),
        title: Set("Mock Work".into()),
        description: Set(None),
        sort_title: Set(None),
        series_type: Set(None),
        language_id: Set(None),
        preferred_edition_id: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .unwrap();

    let ed_id = DbId::new();
    editions::ActiveModel {
        id: Set(ed_id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(Some("Mock Ed".into())),
        published_date: Set(None),
        format_id: Set(None),
        language_id: Set(None),
        notes: Set(None),
        description: Set(None),
        format_metadata: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .unwrap();

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    let res = delete_editions(vec![ed_id], app.state::<AppState>()).await;

    assert!(res.is_ok(), "wrapper failed: {res:?}");
    let outcome = res.unwrap();
    assert_eq!(outcome.deleted, 1, "expected edition deleted");
}