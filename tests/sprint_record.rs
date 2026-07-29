use chrono::{TimeZone, Utc};
use pinto::sprint::SprintId;
use pinto::sprint_record::{SprintRecord, SprintRecordKind};

#[test]
fn a_sprint_record_carries_its_kind_without_changing_identity_or_timestamps() {
    let created = Utc
        .timestamp_opt(1_000, 0)
        .single()
        .expect("valid timestamp");
    let updated = Utc
        .timestamp_opt(2_000, 0)
        .single()
        .expect("valid timestamp");
    let mut record = SprintRecord::new(
        SprintRecordKind::Retro,
        SprintId::new("S-1").expect("valid Sprint ID"),
        "notes",
        created,
    );

    assert_eq!(record.kind, SprintRecordKind::Retro);
    assert_eq!(record.id.as_str(), "S-1");
    assert_eq!(record.body, "notes");
    assert_eq!(record.created, created);
    assert_eq!(record.updated, created);

    record.update_body("revised", updated);

    assert_eq!(record.body, "revised");
    assert_eq!(record.created, created);
    assert_eq!(record.updated, updated);
}
