//! Enrolled Timeline scheduling. Metadata queries return owned values and
//! every native phase finishes before filesystem carrier/trust work.
use super::*;
use crate::error::ShoreError;
use crate::session::derived_access::enrolled_inspector::OwnedInspectorReader;

fn error(value: impl std::fmt::Display) -> ShoreError {
    ShoreError::Message(value.to_string())
}
struct ReadonlyLookup {
    relation: String,
    parameters: Vec<Value>,
}

impl ReadonlyLookup {
    fn new(values: impl IntoIterator<Item = Value>, as_of: TruthCursor) -> Result<Self, String> {
        let mut values = values.into_iter().collect::<Vec<_>>();
        if values.len() > 8192 {
            return Err("Timeline selected lookup limit".into());
        }
        let empty = values.is_empty();
        if empty {
            values.push(Value::Null);
        }
        let body = if empty {
            "SELECT ?3 AS value WHERE 0".into()
        } else {
            format!(
                "SELECT column1 AS value FROM (VALUES {})",
                (0..values.len())
                    .map(|i| format!("(?{})", i + 3))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let mut parameters = vec![
            Value::Integer(to_sql_integer(as_of.epoch)?),
            Value::Integer(to_sql_integer(as_of.sequence)?),
        ];
        parameters.extend(values);
        Ok(Self {
            relation: format!("({body})"),
            parameters,
        })
    }
    fn sql(&self, sql: &str) -> String {
        // These are fixed internal templates, never request-supplied SQL.
        assert_eq!(sql.matches("pointbreak_timeline_lookup").count(), 1);
        sql.replace("pointbreak_timeline_lookup", &self.relation)
    }
}

// Anonymous derived tables retain aliases for correlated predicates without
// introducing a named CTE authorizer context under the enrolled read policy.
fn timeline_relation(as_of: TruthCursor) -> String {
    format!(
        "(
             SELECT locator.sequence,
                    locator.event_id,
                    locator.normalized_occurred_at,
                    event.occurred_at,
                    locator.event_type,
                    locator.track_id,
                    event.actor_id,
                    history.request_state,
                    assessment.assessment,
                    validation.status AS validation_status
             FROM product_history_event AS history
             JOIN locator_event_text AS locator ON locator.sequence = history.sequence
             JOIN semantic_event_fact_text AS event ON event.sequence = history.sequence
             LEFT JOIN semantic_assessment_fact AS assessment
               ON assessment.sequence = history.sequence
             LEFT JOIN semantic_validation_fact AS validation
               ON validation.sequence = history.sequence
             WHERE locator.epoch = {}
               AND locator.sequence <= {}
         ) AS timeline_event",
        as_of.epoch, as_of.sequence
    )
}

fn literal_substring_pattern(value: &str) -> String {
    let mut pattern = String::from("%");
    for ch in value.chars() {
        if matches!(ch, '%' | '_' | '!') {
            pattern.push('!');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

fn enrolled_field_predicate(field: &str, value: &str) -> (String, Vec<Value>) {
    let column = match field {
        "revision" => "reference.revision_id",
        "change" => "correlation.change_id",
        "snapshot" => "reference.object_artifact_content_hash",
        _ => return field_predicate(field, value),
    };
    // These identifiers are canonical text and never contain NUL. LIKE stops
    // at NUL, whereas literal substring matching must not match such a query.
    if value.contains('\0') {
        return ("0 = 1".into(), Vec::new());
    }
    let (sql, _) = field_predicate(field, value);
    (
        sql.replace(
            &format!("instr(lower({column}), ?) > 0"),
            &format!("lower({column}) LIKE ? ESCAPE '!'"),
        ),
        vec![literal_substring_pattern(value).into()],
    )
}

fn enrolled_predicate(
    request: &DerivedTimelinePageRequestV1,
    include_url_types: bool,
) -> TimelinePredicate {
    timeline_predicate_with(request, include_url_types, enrolled_field_predicate)
}

fn select_enrolled_bodyless_timeline(
    db: &rusqlite::Connection,
    as_of: TruthCursor,
    request: &DerivedTimelinePageRequestV1,
) -> Result<BodylessTimelineSelection, TimelinePageError> {
    let relation = timeline_relation(as_of);
    select_bodyless_timeline_from(
        db,
        TimelineSql {
            cte: "",
            relation: &relation,
        },
        request,
        enrolled_predicate(request, true),
        enrolled_predicate(request, false),
    )
}

fn load_timeline_expectations(
    connection: &rusqlite::Connection,
    event_ids: &[String],
    as_of: TruthCursor,
) -> Result<BTreeMap<String, TimelineEventExpectation>, String> {
    let lookup = ReadonlyLookup::new(event_ids.iter().cloned().map(Value::Text), as_of)?;
    let mut expectations = BTreeMap::new();
    let sql = "SELECT locator.epoch, locator.sequence,
                      receipt.logical_reread_key_hash, locator.replay_key,
                      locator.event_id, locator.normalized_occurred_at,
                      locator.event_type, locator.journal_id, locator.subject_id,
                      locator.track_id, locator.payload_hash,
                      receipt.validation_witness, history.request_state,
                      receipt.epoch
               FROM locator_event_text AS locator
               JOIN cursor_receipt_text AS receipt ON receipt.sequence = locator.sequence
               JOIN product_history_event AS history ON history.sequence = locator.sequence
               JOIN pointbreak_timeline_lookup AS selected
                 ON selected.value = locator.event_id
               WHERE locator.epoch = ?1 AND locator.sequence <= ?2
               ORDER BY locator.sequence";
    let mut statement = connection
        .prepare(&lookup.sql(sql))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(lookup.parameters.iter()),
            |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, i64>(13)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (
            epoch,
            sequence,
            logical_reread_key_hash,
            replay_key,
            event_id,
            normalized_occurred_at,
            event_type,
            journal_id,
            subject_id,
            track_id,
            payload_hash,
            validation_witness,
            request_state,
            receipt_epoch,
        ) = row.map_err(|error| error.to_string())?;
        let epoch = to_u64(epoch)?;
        let sequence = to_u64(sequence)?;
        if receipt_epoch != i64::try_from(epoch).map_err(|_| "epoch overflow".to_owned())? {
            return Err(format!(
                "Timeline carrier {event_id} has a mismatched cursor receipt"
            ));
        }
        expectations.insert(
            event_id.clone(),
            TimelineEventExpectation {
                locator: LocatorExpectation {
                    cursor: TruthCursor::new(epoch, sequence),
                    logical_reread_key_hash,
                    replay_key,
                    event_id,
                    normalized_occurred_at,
                    event_type,
                    journal_id,
                    subject_id,
                    track_id,
                    payload_hash,
                    validation_witness,
                },
                request_state,
                tag_values: BTreeSet::new(),
                revision_references: Vec::new(),
                correlations: Vec::new(),
            },
        );
    }
    if expectations.len() != event_ids.len() {
        return Err("selected Timeline rows have missing or duplicate compact locators".to_owned());
    }

    let mut statement = connection
        .prepare(&lookup.sql(
            "SELECT locator.event_id, reference.source_kind,
                    reference.reference_role, reference.resolution,
                    reference.revision_id,
                    reference.object_artifact_content_hash,
                    reference.historical_change_eligible
             FROM product_history_revision_reference AS reference
             JOIN locator_event_text AS locator ON locator.sequence = reference.sequence
             JOIN pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             ORDER BY locator.sequence, reference.source_kind, reference.revision_id",
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(lookup.parameters.iter()),
            |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                Ok((
                    row.get::<_, String>(0)?,
                    NormalizedRevisionReference {
                        source_kind: row.get(1)?,
                        reference_role: row.get(2)?,
                        resolution: row.get(3)?,
                        revision_id: row.get(4)?,
                        object_artifact_content_hash: row.get(5)?,
                        historical_change_eligible: row.get::<_, i64>(6)? != 0,
                    },
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (event_id, reference) = row.map_err(|error| error.to_string())?;
        expectations
            .get_mut(&event_id)
            .ok_or_else(|| format!("Revision reference selects unknown event {event_id}"))?
            .revision_references
            .push(reference);
    }

    let mut statement = connection
        .prepare(&lookup.sql(
            "SELECT locator.event_id, correlation.change_id,
                    correlation.correlation_role, correlation.source_kind,
                    correlation.source_id, correlation.support_sequence
             FROM product_history_change_correlation AS correlation
             JOIN locator_event_text AS locator ON locator.sequence = correlation.sequence
             JOIN pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             ORDER BY locator.sequence, correlation.change_id,
                      correlation.correlation_role, correlation.source_kind,
                      correlation.source_id, correlation.support_sequence",
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(lookup.parameters.iter()),
            |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                Ok((
                    row.get::<_, String>(0)?,
                    NormalizedChangeCorrelation {
                        change_id: row.get(1)?,
                        correlation_role: row.get(2)?,
                        source_kind: row.get(3)?,
                        source_id: row.get(4)?,
                        support_sequence: to_u64_sql(row.get(5)?)?,
                    },
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (event_id, correlation) = row.map_err(|error| error.to_string())?;
        expectations
            .get_mut(&event_id)
            .ok_or_else(|| format!("Change correlation selects unknown event {event_id}"))?
            .correlations
            .push(correlation);
    }

    let mut statement = connection
        .prepare(&lookup.sql(
            "SELECT locator.event_id, tag.tag_value
             FROM product_history_tag_value AS tag
             JOIN locator_event_text AS locator ON locator.sequence = tag.sequence
             JOIN pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             ORDER BY locator.sequence, tag.tag_value",
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(lookup.parameters.iter()),
            |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            },
        )
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (event_id, tag) = row.map_err(|error| error.to_string())?;
        expectations
            .get_mut(&event_id)
            .ok_or_else(|| format!("Timeline tag selects unknown event {event_id}"))?
            .tag_values
            .insert(tag);
    }
    Ok(expectations)
}

fn query_candidate_proposal_locators(
    connection: &rusqlite::Connection,
    revision_ids: &BTreeSet<String>,
    as_of: TruthCursor,
) -> Result<BTreeMap<String, Vec<ProposalCarrierLocator>>, String> {
    if revision_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let lookup = ReadonlyLookup::new(revision_ids.iter().cloned().map(Value::Text), as_of)?;
    let mut statement = connection
        .prepare(&lookup.sql(
            "SELECT locator.epoch, locator.sequence,
                    receipt.logical_reread_key_hash, locator.replay_key,
                    locator.event_id, locator.event_type, locator.payload_hash,
                    receipt.validation_witness, proposal.revision_id,
                    proposal.object_artifact_content_hash, receipt.epoch
             FROM semantic_revision_proposal_carrier AS proposal
             JOIN locator_event_text AS locator ON locator.sequence = proposal.sequence
             JOIN cursor_receipt_text AS receipt ON receipt.sequence = proposal.sequence
             JOIN pointbreak_timeline_lookup AS selected
               ON selected.value = proposal.revision_id
             WHERE locator.epoch = ?1 AND locator.sequence <= ?2
             ORDER BY proposal.revision_id, locator.sequence",
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(lookup.parameters.iter()),
            |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let mut grouped = revision_ids
        .iter()
        .cloned()
        .map(|revision_id| (revision_id, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for row in rows {
        let (
            epoch,
            sequence,
            logical_reread_key_hash,
            replay_key,
            event_id,
            event_type,
            payload_hash,
            validation_witness,
            revision_id,
            artifact_hash,
            receipt_epoch,
        ) = row.map_err(|error| error.to_string())?;
        let epoch = to_u64(epoch)?;
        if receipt_epoch != i64::try_from(epoch).map_err(|_| "epoch overflow".to_owned())? {
            return Err(format!(
                "proposal carrier {event_id} has a mismatched receipt epoch"
            ));
        }
        let revision = RevisionRefV1::new(RevisionId::new(revision_id.clone()), artifact_hash)
            .map_err(|error| error.to_string())?;
        grouped
            .get_mut(&revision_id)
            .ok_or_else(|| format!("proposal carrier selects unexpected Revision {revision_id}"))?
            .push(ProposalCarrierLocator {
                cursor: TruthCursor::new(epoch, to_u64(sequence)?),
                logical_reread_key_hash,
                replay_key,
                event_id: EventId::new(event_id),
                event_type,
                payload_hash,
                validation_witness,
                revision,
            });
    }
    Ok(grouped)
}

fn query_locator_expectations(
    connection: &rusqlite::Connection,
    event_ids: &[String],
    as_of: TruthCursor,
) -> Result<BTreeMap<String, LocatorExpectation>, String> {
    let lookup = ReadonlyLookup::new(event_ids.iter().cloned().map(Value::Text), as_of)?;
    let mut statement = connection
        .prepare(&lookup.sql(
            "SELECT locator.epoch, locator.sequence,
                    receipt.logical_reread_key_hash, locator.replay_key,
                    locator.event_id, locator.normalized_occurred_at,
                    locator.event_type, locator.journal_id, locator.subject_id,
                    locator.track_id, locator.payload_hash,
                    receipt.validation_witness, receipt.epoch
             FROM locator_event_text AS locator
             JOIN cursor_receipt_text AS receipt ON receipt.sequence = locator.sequence
             JOIN pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             WHERE locator.epoch = ?1 AND locator.sequence <= ?2
             ORDER BY locator.event_id",
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(lookup.parameters.iter()),
            |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, i64>(12)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let mut expectations = BTreeMap::new();
    for row in rows {
        let (
            epoch,
            sequence,
            logical_reread_key_hash,
            replay_key,
            event_id,
            normalized_occurred_at,
            event_type,
            journal_id,
            subject_id,
            track_id,
            payload_hash,
            validation_witness,
            receipt_epoch,
        ) = row.map_err(|error| error.to_string())?;
        if receipt_epoch != epoch {
            return Err(format!(
                "Timeline carrier {event_id} has a mismatched receipt epoch"
            ));
        }
        expectations.insert(
            event_id.clone(),
            LocatorExpectation {
                cursor: TruthCursor::new(to_u64(epoch)?, to_u64(sequence)?),
                logical_reread_key_hash,
                replay_key,
                event_id,
                normalized_occurred_at,
                event_type,
                journal_id,
                subject_id,
                track_id,
                payload_hash,
                validation_witness,
            },
        );
    }
    if expectations.len() != event_ids.len() {
        return Err("Timeline support has missing or duplicate compact locators".to_owned());
    }
    Ok(expectations)
}

fn validate_signature_support(
    connection: &rusqlite::Connection,
    support_plan: &SupportEventPlan,
    primary_by_id: &BTreeMap<String, ShoreEvent>,
    secondary: &[HydratedLocatorRow],
    as_of: TruthCursor,
) -> Result<(), String> {
    let secondary_by_id = secondary
        .iter()
        .map(|hydrated| {
            (
                hydrated.event.event_id.as_str().to_owned(),
                (&hydrated.event, hydrated.row.cursor.sequence),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let valid_targets = primary_by_id
        .keys()
        .chain(support_plan.removal_event_ids.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    if support_plan.signature_event_ids.is_empty() {
        return Ok(());
    }
    let lookup = ReadonlyLookup::new(
        support_plan
            .signature_event_ids
            .iter()
            .cloned()
            .map(Value::Text),
        as_of,
    )?;
    let mut statement = connection
        .prepare(&lookup.sql(
            "SELECT locator.event_id, locator.sequence, signature.target_event_id
             FROM product_history_signature AS signature
             JOIN locator_event_text AS locator ON locator.sequence = signature.sequence
             JOIN pointbreak_timeline_lookup AS selected
               ON selected.value = locator.event_id
             WHERE locator.epoch = ?1 AND locator.sequence <= ?2
             ORDER BY locator.event_id",
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(lookup.parameters.iter()),
            |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                Ok((
                    row.get::<_, String>(0)?,
                    to_u64_sql(row.get(1)?)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if rows.len() != support_plan.signature_event_ids.len() {
        return Err("Timeline signature support row is absent".to_owned());
    }
    for (event_id, sequence, target_event_id) in rows {
        if !valid_targets.contains(&target_event_id) {
            return Err(format!(
                "Timeline signature carrier {event_id} targets an unselected event"
            ));
        }
        let (event, hydrated_sequence) = secondary_by_id
            .get(&event_id)
            .copied()
            .or_else(|| {
                primary_by_id.get(&event_id).map(|event| {
                    // A signature event cannot ordinarily be a selected Timeline
                    // row, but a duplicate support identity still validates
                    // through its normalized sequence below.
                    (event, sequence)
                })
            })
            .ok_or_else(|| format!("Timeline signature carrier {event_id} is absent"))?;
        if hydrated_sequence != sequence {
            return Err(format!("Timeline signature carrier {event_id} moved"));
        }
        let fact =
            ProductHistoryFact::from_event(sequence, event).map_err(|error| error.to_string())?;
        if fact.signature_target_event_id.as_deref() != Some(target_event_id.as_str()) {
            return Err(format!(
                "Timeline signature carrier {event_id} has the wrong target"
            ));
        }
    }
    Ok(())
}

fn select_first_level_support(
    connection: &rusqlite::Connection,
    expectations: &BTreeMap<String, TimelineEventExpectation>,
    as_of: TruthCursor,
) -> Result<TimelineSupportSelection, String> {
    let candidate_revision_ids = expectations
        .values()
        .flat_map(|expectation| &expectation.revision_references)
        .filter(|reference| reference.reference_role == "candidate")
        .map(|reference| reference.revision_id.clone())
        .collect::<BTreeSet<_>>();
    let candidate_proposals =
        query_candidate_proposal_locators(connection, &candidate_revision_ids, as_of)?;
    let support_sequences = expectations
        .values()
        .flat_map(|expectation| &expectation.correlations)
        .map(|correlation| correlation.support_sequence)
        .collect::<BTreeSet<_>>();
    let correlation_support_event_ids =
        event_ids_for_sequences(connection, &support_sequences, as_of)?;
    Ok(TimelineSupportSelection {
        candidate_proposals,
        correlation_support_event_ids,
    })
}

fn event_ids_for_sequences(
    connection: &rusqlite::Connection,
    sequences: &BTreeSet<u64>,
    as_of: TruthCursor,
) -> Result<BTreeSet<String>, String> {
    let values = sequences
        .iter()
        .map(|value| to_sql_integer(*value).map(Value::Integer))
        .collect::<Result<Vec<_>, _>>()?;
    let lookup = ReadonlyLookup::new(values, as_of)?;
    let mut statement = connection.prepare(&lookup.sql("SELECT locator.event_id FROM locator_event_text AS locator JOIN pointbreak_timeline_lookup AS selected ON selected.value=locator.sequence WHERE locator.epoch=?1 AND locator.sequence<=?2 ORDER BY locator.event_id")).map_err(|e|e.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params_from_iter(lookup.parameters.iter()),
            |row| {
                crate::session::derived_access::owned_row_charge(row)?;
                row.get::<_, String>(0)
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|e| e.to_string())?;
    if rows.len() != sequences.len() {
        return Err("Timeline correlation support sequence is absent".into());
    }
    Ok(rows)
}

fn hydrate_validated(
    reader: &mut OwnedInspectorReader<'_>,
    event_ids: &[String],
    as_of: TruthCursor,
) -> Result<Vec<HydratedLocatorRow>, TimelinePageError> {
    if event_ids.len() > 8192 {
        return Err("Timeline selected carrier limit".to_owned().into());
    }
    let (expectations, rows) = reader
        .pin
        .phase(|db, checkpoint, _| {
            if checkpoint.truth_cursor != as_of {
                return Err(error("Timeline request cursor changed"));
            }
            Ok((
                query_locator_expectations(db, event_ids, as_of).map_err(error)?,
                super::super::sqlite::SqliteLocator::selected_rows_on(db, event_ids, as_of)
                    .map_err(error)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let rows = reader
        .hydrate_detached(event_ids, rows)
        .map_err(|e| e.to_string())?;
    for (id, row) in event_ids.iter().zip(&rows) {
        expectations
            .get(id)
            .ok_or_else(|| format!("compact Timeline locator {id} is absent"))?
            .validate(row)?;
    }
    Ok(rows)
}

fn validate_timeline_carriers(
    reader: &mut OwnedInspectorReader<'_>,
    selected_event_ids: &[String],
    as_of: TruthCursor,
    hook: &mut impl FnMut(TimelineReadBoundary),
) -> Result<ValidatedTimelineCarriers, TimelinePageError> {
    hook(TimelineReadBoundary::SupportExpansionStarted);
    let (selected_expectations, support) = reader
        .pin
        .phase(|db, _, _| {
            let expected =
                load_timeline_expectations(db, selected_event_ids, as_of).map_err(error)?;
            let support = select_first_level_support(db, &expected, as_of).map_err(error)?;
            Ok((expected, support))
        })
        .map_err(|e| e.to_string())?;
    let mut primary_event_ids = selected_event_ids.iter().cloned().collect::<BTreeSet<_>>();
    primary_event_ids.extend(support.correlation_support_event_ids.iter().cloned());
    primary_event_ids.extend(
        support
            .candidate_proposals
            .values()
            .flatten()
            .map(|locator| locator.event_id.as_str().to_owned()),
    );
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        let selected = selected_event_ids.iter().cloned().collect::<BTreeSet<_>>();
        let candidates = support
            .candidate_proposals
            .values()
            .flatten()
            .map(|locator| locator.event_id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let candidate_only = candidates
            .difference(&selected)
            .cloned()
            .collect::<BTreeSet<_>>();
        let selected_or_candidate = selected
            .union(&candidates)
            .cloned()
            .collect::<BTreeSet<_>>();
        let correlation_only = support
            .correlation_support_event_ids
            .difference(&selected_or_candidate)
            .count();
        record_timeline_selected_carriers(selected.len());
        record_timeline_revision_candidate_carriers(candidate_only.len());
        record_timeline_correlation_support_carriers(correlation_only);
    }
    let primary_event_ids = primary_event_ids.into_iter().collect::<Vec<_>>();
    hook(TimelineReadBoundary::CarrierLocatorsSelected);
    #[cfg(any(test, feature = "longitudinal-counting"))]
    reach_timeline_carrier_locators_selected_v1().map_err(TimelinePageError::Invalid)?;
    let primary_hydrated = hydrate_validated(reader, &primary_event_ids, as_of)?;
    hook(TimelineReadBoundary::CarrierHydrationMidpoint);
    let primary_by_id = primary_hydrated
        .iter()
        .map(|hydrated| {
            (
                hydrated.event.event_id.as_str().to_owned(),
                hydrated.event.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let primary_sequence_by_id = primary_hydrated
        .iter()
        .map(|hydrated| {
            (
                hydrated.event.event_id.as_str().to_owned(),
                hydrated.row.cursor.sequence,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let primary_locator_by_id = primary_hydrated
        .iter()
        .map(|hydrated| (hydrated.event.event_id.as_str().to_owned(), &hydrated.row))
        .collect::<BTreeMap<_, _>>();

    let mut candidate_bindings = BTreeMap::new();
    for (revision_id, locators) in &support.candidate_proposals {
        let mut bindings = BTreeSet::new();
        for locator in locators {
            let event_id = locator.event_id.as_str();
            let event = primary_by_id
                .get(event_id)
                .ok_or_else(|| format!("candidate proposal carrier {event_id} is absent"))?;
            let row = primary_locator_by_id
                .get(event_id)
                .ok_or_else(|| format!("candidate proposal locator {event_id} is absent"))?;
            if row.cursor != locator.cursor
                || sha256_bytes_hex(row.logical_reread_key.as_bytes())
                    != locator.logical_reread_key_hash
                || row.replay_key != locator.replay_key
                || row.event_type != locator.event_type
                || row.payload_hash != locator.payload_hash
                || row.validation_witness != locator.validation_witness
            {
                return Err(format!(
                    "candidate proposal carrier {event_id} differs from its compact Revision locator"
                )
                .into());
            }
            let actual = super::super::changes::exact_revision_from_proposal(event)
                .map_err(|error| error.to_string())?;
            if actual != locator.revision || actual.revision_id.as_str() != revision_id {
                return Err(format!(
                    "candidate proposal carrier {event_id} has the wrong exact Revision binding"
                )
                .into());
            }
            bindings.insert(actual);
        }
        candidate_bindings.insert(revision_id.clone(), bindings);
    }

    let primary_events = primary_hydrated
        .iter()
        .map(|hydrated| hydrated.event.clone())
        .collect::<Vec<_>>();
    let support_plan = reader
        .pin
        .phase(|db, _, _| {
            super::super::support::support_event_plan_readonly(db, &primary_events, as_of)
                .map_err(error)
        })
        .map_err(|e| e.to_string())?;
    let secondary_event_ids = support_plan
        .removal_event_ids
        .iter()
        .chain(&support_plan.signature_event_ids)
        .filter(|event_id| !primary_by_id.contains_key(*event_id))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        let primary = primary_by_id.keys().cloned().collect::<BTreeSet<_>>();
        let removal_only = support_plan
            .removal_event_ids
            .iter()
            .filter(|event_id| !primary.contains(*event_id))
            .cloned()
            .collect::<BTreeSet<_>>();
        let primary_or_removal = primary
            .union(&removal_only)
            .cloned()
            .collect::<BTreeSet<_>>();
        let signature_only = support_plan
            .signature_event_ids
            .iter()
            .filter(|event_id| !primary_or_removal.contains(*event_id))
            .count();
        record_timeline_removal_support_carriers(removal_only.len());
        record_timeline_signature_support_carriers(signature_only);
    }
    let secondary_hydrated = hydrate_validated(reader, &secondary_event_ids, as_of)?;
    reader
        .pin
        .phase(|db, _, _| {
            validate_signature_support(
                db,
                &support_plan,
                &primary_by_id,
                &secondary_hydrated,
                as_of,
            )
            .map_err(error)
        })
        .map_err(|e| e.to_string())?;
    // Reuse the complete payload/target-record-hash validation in addition to
    // the Timeline-specific compact signature row comparison.
    let secondary_events = secondary_hydrated
        .iter()
        .map(|row| row.event.clone())
        .chain(
            primary_events
                .iter()
                .filter(|event| {
                    support_plan
                        .all_event_ids()
                        .contains(&event.event_id.as_str().to_owned())
                })
                .cloned(),
        )
        .collect::<Vec<_>>();
    super::super::fact_reads::validate_support_events(
        &support_plan,
        &primary_events,
        &secondary_events,
    )
    .map_err(TimelinePageError::Invalid)?;

    let selected = selected_event_ids
        .iter()
        .map(|event_id| {
            primary_by_id
                .get(event_id)
                .cloned()
                .ok_or_else(|| format!("selected Timeline carrier {event_id} is absent"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ValidatedTimelineCarriers {
        selected,
        selected_expectations,
        primary_by_id,
        primary_sequence_by_id,
        candidate_bindings,
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_bodyless_page(
    reader: &mut OwnedInspectorReader<'_>,
    change_projection: &crate::session::ChangeDocumentProjectionV1,
    as_of: TruthCursor,
    authority_cursor: AuthorityCursorV2,
    source_change_projection_stamp: String,
    timeline_projection_stamp: String,
    request: &DerivedTimelinePageRequestV1,
    trust_set: &TrustSet,
    global: TimelineGlobalProjection,
    hook: &mut impl FnMut(TimelineReadBoundary),
) -> Result<DerivedTimelinePageV1, TimelinePageError> {
    let selection = reader
        .pin
        .phase(|db, _, _| select_enrolled_bodyless_timeline(db, as_of, request).map_err(error))
        .map_err(|e| e.to_string())?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        record_timeline_sqlite_candidates(selection.match_count);
        record_timeline_sqlite_window_rows(selection.keys.len());
        record_timeline_sqlite_facet_rows(selection.facets.values().sum());
    }
    let event_ids = selection
        .keys
        .iter()
        .map(|key| key.event_id.clone())
        .collect::<Vec<_>>();
    let carriers = validate_timeline_carriers(reader, &event_ids, as_of, hook)?;
    let (drafts, selected_diagnostics) =
        project_selected_event_history_without_trust(&carriers.selected, change_projection)
            .map_err(|error| error.to_string())?;
    if !selected_diagnostics
        .iter()
        .all(|diagnostic| global.diagnostics.contains(diagnostic))
    {
        return Err(
            "selected Timeline diagnostics disagree with normalized history"
                .to_owned()
                .into(),
        );
    }
    validate_selected_relations(&carriers, &drafts)?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_timeline_trust_support_carriers(drafts.len());
    let mut entries =
        bind_selected_event_history_trust(drafts, trust_set).map_err(|error| error.to_string())?;
    hook(TimelineReadBoundary::TrustBindingComplete);
    reader
        .pin
        .phase(|db, _, _| attach_derived_relation_targets(db, as_of, &mut entries).map_err(error))
        .map_err(|e| e.to_string())?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_timeline_entries_emitted(entries.len());
    let document = EventHistoryDocumentV1 {
        schema: INSPECT_EVENT_HISTORY_SCHEMA.to_owned(),
        version: 1,
        event_count: authority_cursor.event_count,
        authority_cursor,
        source_change_projection_stamp,
        timeline_projection_stamp,
        order: event_history_order(request.order()),
        match_count: selection.match_count,
        offset: selection.offset,
        match_index: selection.match_index,
        facets: selection.facets,
        completion: global.completion,
        diagnostics: global.diagnostics,
        query_notices: selection.query_notices,
        entries,
        previous: None,
        next: None,
    };
    Ok(DerivedTimelinePageV1::new(document, selection.adjacent))
}

#[allow(clippy::too_many_arguments)]
fn prepare_exhaustive_page(
    reader: &mut OwnedInspectorReader<'_>,
    change_projection: &crate::session::ChangeDocumentProjectionV1,
    as_of: TruthCursor,
    authority_cursor: AuthorityCursorV2,
    source_change_projection_stamp: String,
    timeline_projection_stamp: String,
    request: &DerivedTimelinePageRequestV1,
    trust_set: &TrustSet,
    global: TimelineGlobalProjection,
    hook: &mut impl FnMut(TimelineReadBoundary),
) -> Result<DerivedTimelinePageV1, TimelinePageError> {
    let (_candidate_count, keys) = reader
        .pin
        .phase(|db, _, _| {
            (|| -> Result<_, String> {
                let relation = timeline_relation(as_of);
                let candidates = enrolled_predicate(request, false);
                let candidate_count = query_selected_count(
                    db,
                    "",
                    &relation,
                    &candidates.sql,
                    &candidates.parameters,
                )?;
                if candidate_count > 8192 {
                    return Err("Timeline exhaustive candidate limit".to_owned());
                }
                let keys = query_selected_window(
                    db,
                    "",
                    &relation,
                    &candidates.sql,
                    &candidates.parameters,
                    false,
                    candidate_count,
                    0,
                )?;
                Ok((candidate_count, keys))
            })()
            .map_err(error)
        })
        .map_err(|e| e.to_string())?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    {
        record_timeline_sqlite_candidates(_candidate_count);
        record_timeline_sqlite_window_rows(keys.len());
        record_timeline_sqlite_facet_rows(0);
        record_timeline_exhaustive_candidates(keys.len());
    }
    let event_ids = keys
        .iter()
        .map(|key| key.event_id.clone())
        .collect::<Vec<_>>();
    let carriers = validate_timeline_carriers(reader, &event_ids, as_of, hook)?;
    let (drafts, selected_diagnostics) =
        project_selected_event_history_without_trust(&carriers.selected, change_projection)
            .map_err(|error| error.to_string())?;
    if !selected_diagnostics
        .iter()
        .all(|diagnostic| global.diagnostics.contains(diagnostic))
    {
        return Err(
            "exhaustive Timeline diagnostics disagree with normalized history"
                .to_owned()
                .into(),
        );
    }
    validate_selected_relations(&carriers, &drafts)?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_timeline_trust_support_carriers(drafts.len());
    let entries =
        bind_selected_event_history_trust(drafts, trust_set).map_err(|error| error.to_string())?;
    hook(TimelineReadBoundary::TrustBindingComplete);
    let parsed = parse_search_query_for(
        request.query().unwrap_or_default(),
        QuerySurface::ChangeTimeline,
    );
    let facet_base = entries
        .into_iter()
        .filter(|entry| matches_query(&event_history_search_record(entry), &parsed.clauses))
        .collect::<Vec<_>>();
    let facets = facet_base
        .iter()
        .fold(BTreeMap::new(), |mut facets, entry| {
            *facets
                .entry(entry.event_type.as_str().to_owned())
                .or_insert(0) += 1;
            facets
        });
    let mut selected = facet_base
        .into_iter()
        .filter(|entry| {
            request.event_types().is_empty() || request.event_types().contains(&entry.event_type)
        })
        .collect::<Vec<_>>();
    if request.order() == DerivedTimelineOrderV1::Desc {
        selected.reverse();
    }
    let match_count = selected.len();
    let (offset, match_index) = exhaustive_offset(&selected, request)?;
    let end = offset.saturating_add(request.limit()).min(match_count);
    let adjacent = exhaustive_adjacent(&selected, request.limit(), offset, end)?;
    let mut entries = selected[offset..end].to_vec();
    reader
        .pin
        .phase(|db, _, _| attach_derived_relation_targets(db, as_of, &mut entries).map_err(error))
        .map_err(|e| e.to_string())?;
    #[cfg(any(test, feature = "longitudinal-counting"))]
    record_timeline_entries_emitted(entries.len());
    let document = EventHistoryDocumentV1 {
        schema: INSPECT_EVENT_HISTORY_SCHEMA.to_owned(),
        version: 1,
        event_count: authority_cursor.event_count,
        authority_cursor,
        source_change_projection_stamp,
        timeline_projection_stamp,
        order: event_history_order(request.order()),
        match_count,
        offset,
        match_index,
        facets,
        completion: global.completion,
        diagnostics: global.diagnostics,
        query_notices: parsed
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect(),
        entries,
        previous: None,
        next: None,
    };
    Ok(DerivedTimelinePageV1::new(document, adjacent))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::session::derived_access) fn prepare_timeline_page(
    reader: &mut OwnedInspectorReader<'_>,
    change_projection: &crate::session::ChangeDocumentProjectionV1,
    as_of: TruthCursor,
    authority_cursor: AuthorityCursorV2,
    source_change_projection_stamp: String,
    timeline_projection_stamp: String,
    request: &DerivedTimelinePageRequestV1,
    trust_set: &TrustSet,
    hook: &mut impl FnMut(TimelineReadBoundary),
) -> Result<DerivedTimelinePageV1, TimelinePageError> {
    let global = reader
        .pin
        .phase(|db, _, _| {
            let relation = timeline_relation(as_of);
            query_global_projection_from(
                db,
                as_of,
                TimelineSql {
                    cte: "",
                    relation: &relation,
                },
            )
            .map_err(error)
        })
        .map_err(|e| e.to_string())?;
    match classify_query(request) {
        TimelineQueryClass::Bodyless => prepare_bodyless_page(
            reader,
            change_projection,
            as_of,
            authority_cursor,
            source_change_projection_stamp,
            timeline_projection_stamp,
            request,
            trust_set,
            global,
            hook,
        ),
        TimelineQueryClass::Exhaustive => prepare_exhaustive_page(
            reader,
            change_projection,
            as_of,
            authority_cursor,
            source_change_projection_stamp,
            timeline_projection_stamp,
            request,
            trust_set,
            global,
            hook,
        ),
    }
}

#[cfg(test)]
mod lookup_tests {
    use super::*;

    #[test]
    fn readonly_lookup_preserves_cursor_slots_and_empty_set_semantics() {
        let cursor = TruthCursor::new(7, 11);
        let empty = ReadonlyLookup::new([], cursor).unwrap();
        assert!(
            empty
                .sql("SELECT value FROM pointbreak_timeline_lookup")
                .contains("SELECT ?3 AS value WHERE 0")
        );
        assert_eq!(
            empty.parameters,
            [Value::Integer(7), Value::Integer(11), Value::Null]
        );
        let values = [
            Value::Text("a'quoted-id".into()),
            Value::Text("second".into()),
        ];
        let lookup = ReadonlyLookup::new(values.clone(), cursor).unwrap();
        assert!(lookup.relation.contains("VALUES (?3),(?4)"));
        assert!(!lookup.relation.contains("quoted-id"));
        assert_eq!(&lookup.parameters[2..], &values);
        let single = ReadonlyLookup::new([Value::Integer(19)], cursor).unwrap();
        assert_eq!(
            single.relation,
            "(SELECT column1 AS value FROM (VALUES (?3)))"
        );
        assert_eq!(
            single.parameters,
            [Value::Integer(7), Value::Integer(11), Value::Integer(19)]
        );
        let full = ReadonlyLookup::new((0..8192).map(Value::Integer), cursor).unwrap();
        assert_eq!(full.parameters.len(), 8194);
        assert!(full.relation.ends_with("(?8194)))"));
        for lookup in [&empty, &single, &lookup, &full] {
            let sql = lookup.sql("SELECT selected.value FROM pointbreak_timeline_lookup AS selected WHERE ?1 = 7 AND ?2 = 11");
            assert!(!sql.contains("WITH "));
            assert!(!sql.contains("pointbreak_timeline_lookup"));
            assert!(sql.contains(" AS selected WHERE ?1 = 7 AND ?2 = 11"));
        }
        assert!(ReadonlyLookup::new((0..8193).map(Value::Integer), cursor).is_err());
    }

    #[test]
    fn enrolled_sql_preserves_relation_and_literal_substring_semantics() {
        let cursor = TruthCursor::new(7, 11);
        assert_eq!(
            timeline_relation(cursor),
            format!(
                "{} AS timeline_event",
                timeline_cte(cursor)
                    .strip_prefix("WITH timeline_event AS ")
                    .unwrap()
            ),
        );
        for field in ["revision", "change", "snapshot"] {
            for (value, pattern) in [
                ("abc", "%abc%"),
                ("a%b_c!d", "%a!%b!_c!!d%"),
                (r"a\b'c", r"%a\b'c%"),
                ("é", "%é%"),
                ("", "%%"),
            ] {
                let (sql, parameters) = enrolled_field_predicate(field, value);
                assert!(sql.contains(" LIKE ? ESCAPE '!'"));
                assert!(!sql.contains("instr("));
                assert_eq!(parameters, [Value::Text(pattern.into())]);
                let (old, old_parameters) = field_predicate(field, value);
                assert!(old.contains("instr("));
                assert_eq!(old_parameters, [Value::Text(value.into())]);
                if field == "snapshot" {
                    assert!(sql.contains("reference.resolution = 'exact'"));
                }
            }
            assert_eq!(
                enrolled_field_predicate(field, "abc\0def"),
                ("0 = 1".into(), vec![])
            );
        }
        for field in [
            "type",
            "check",
            "assessment",
            "track",
            "actor",
            "is",
            "tag",
            "before",
            "after",
        ] {
            assert_eq!(
                enrolled_field_predicate(field, "abc"),
                field_predicate(field, "abc")
            );
        }
        let request = DerivedTimelinePageRequestV1 {
            limit: 50,
            order: DerivedTimelineOrderV1::Asc,
            query: Some("revision:ABC -change:a%b snapshot:x_y".into()),
            event_types: vec![],
            track: Some("chosen-track".into()),
            change: None,
            revision: None,
            position: DerivedTimelinePagePositionV1::Initial,
        };
        let predicate = enrolled_predicate(&request, true);
        assert_eq!(
            predicate.parameters,
            ["chosen-track", "%abc%", "%a!%b%", "%x!_y%"].map(|s| Value::Text(s.into()))
        );
        assert!(predicate.sql.contains("NOT (EXISTS"));
        assert!(!predicate.sql.contains("instr("));
        assert_eq!(
            predicate.notices,
            timeline_predicate(&request, true).notices
        );
    }
}
