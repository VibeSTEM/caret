use super::{HostCaretKind, HostCaretPosition, HostCaretTransition};
use crate::editing::Direction;
use caret_doc::NodeId;

#[test]
fn horizontal_edges_use_three_states_for_empty_and_nonempty_hosts() {
    let host_id = NodeId::generate();
    let before = HostCaretPosition::<usize>::Before {
        host_id: host_id.clone(),
        kind: HostCaretKind::Block,
    };
    let after = HostCaretPosition::<usize>::After {
        host_id: host_id.clone(),
        kind: HostCaretKind::Block,
    };
    let empty_inside = HostCaretPosition::Inside {
        host_id: host_id.clone(),
        kind: HostCaretKind::Block,
        local: 0,
    };
    let nonempty_start = HostCaretPosition::Inside {
        host_id: host_id.clone(),
        kind: HostCaretKind::Block,
        local: 0,
    };
    let nonempty_middle = HostCaretPosition::Inside {
        host_id: host_id.clone(),
        kind: HostCaretKind::Block,
        local: 1,
    };
    let nonempty_end = HostCaretPosition::Inside {
        host_id,
        kind: HostCaretKind::Block,
        local: 2,
    };

    assert_eq!(
        before.horizontal_transition(true, false, false),
        HostCaretTransition::EnterStart
    );
    assert_eq!(
        after.horizontal_transition(false, false, false),
        HostCaretTransition::EnterEnd
    );
    assert_eq!(
        empty_inside.horizontal_transition(false, true, true),
        HostCaretTransition::LeaveBefore
    );
    assert_eq!(
        empty_inside.horizontal_transition(true, true, true),
        HostCaretTransition::LeaveAfter
    );
    assert_eq!(
        nonempty_start.horizontal_transition(false, true, false),
        HostCaretTransition::LeaveBefore
    );
    assert_eq!(
        nonempty_middle.horizontal_transition(true, false, false),
        HostCaretTransition::MoveInside
    );
    assert_eq!(
        nonempty_end.horizontal_transition(true, false, true),
        HostCaretTransition::LeaveAfter
    );
    assert_eq!(before.outside_direction(), Some(Direction::Backward));
    assert_eq!(after.outside_direction(), Some(Direction::Forward));
    assert_eq!(nonempty_middle.outside_direction(), None);
}
