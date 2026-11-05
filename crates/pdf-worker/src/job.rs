//! Synthetic job used to exercise budgets and cancellation without reading a PDF.

use pdf_syntax::{Budget, Error};

/// Processes `units` work units, charging one object per unit and checking
/// cancellation before each one. `on_unit` receives the number of completed
/// units (progress reporting, and a deterministic hook for tests).
pub fn run_synthetic_job(
    units: u64,
    budget: &mut Budget,
    mut on_unit: impl FnMut(u64),
) -> Result<u64, Error> {
    let mut done = 0u64;
    while done < units {
        budget.check_cancelled()?;
        budget.charge_object()?;
        done = done.saturating_add(1);
        on_unit(done);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_syntax::{CancellationToken, LimitKind, ResourceLimits};

    fn budget_with(max_objects_visited: u64, token: CancellationToken) -> Budget {
        let limits = ResourceLimits {
            max_objects_visited,
            ..ResourceLimits::default()
        };
        Budget::new(limits, token)
    }

    #[test]
    fn completes_within_budget() {
        let mut budget = budget_with(5, CancellationToken::new());
        assert_eq!(run_synthetic_job(5, &mut budget, |_| {}), Ok(5));
    }

    #[test]
    fn stops_at_limit_plus_one() {
        let mut budget = budget_with(5, CancellationToken::new());
        assert_eq!(
            run_synthetic_job(6, &mut budget, |_| {}),
            Err(Error::LimitExceeded {
                kind: LimitKind::ObjectsVisited,
                limit: 5
            })
        );
        assert_eq!(budget.objects_visited(), 5);
    }

    #[test]
    fn pre_cancelled_job_does_no_work() {
        let token = CancellationToken::new();
        token.cancel();
        let mut budget = budget_with(5, token);
        assert_eq!(
            run_synthetic_job(3, &mut budget, |_| {}),
            Err(Error::Cancelled)
        );
        assert_eq!(budget.objects_visited(), 0);
    }

    #[test]
    fn cancellation_during_run_stops_the_next_unit() {
        let token = CancellationToken::new();
        let mut budget = budget_with(100, token.clone());
        let mut last = 0;
        let result = run_synthetic_job(10, &mut budget, |done| {
            last = done;
            if done == 3 {
                token.cancel();
            }
        });
        assert_eq!(result, Err(Error::Cancelled));
        assert_eq!(last, 3);
        assert_eq!(budget.objects_visited(), 3);
    }
}
