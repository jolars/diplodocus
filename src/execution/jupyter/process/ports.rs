//! Keep selected kernel ports owned across the listener-to-child handoff.
//!
//! This registry coordinates this process's launches; external processes can still race.

use std::collections::BTreeSet;
use std::future::Future;
use std::io;
use std::net::Ipv4Addr;
use std::ops::RangeInclusive;
use std::sync::Mutex;

use tokio::net::TcpListener;

static LEASED_PORTS: Mutex<BTreeSet<u16>> = Mutex::new(BTreeSet::new());
const MAX_CANDIDATES: usize = 128;
const FIRST_PORT: u32 = 1024;
const PORT_COUNT: u32 = u16::MAX as u32 + 1 - FIRST_PORT;

pub(super) struct PortLease<'registry> {
    registry: &'registry Mutex<BTreeSet<u16>>,
    ports: Vec<u16>,
}

impl PortLease<'_> {
    pub(super) fn ports(&self) -> &[u16] {
        &self.ports
    }

    fn release_last(&mut self) {
        if let Some(port) = self.ports.pop() {
            self.registry
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&port);
        }
    }
}

impl Drop for PortLease<'_> {
    fn drop(&mut self) {
        let mut leased = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for port in &self.ports {
            leased.remove(port);
        }
    }
}

fn ephemeral_range(text: &str) -> io::Result<RangeInclusive<u16>> {
    let values: Vec<_> = text.split_whitespace().collect();
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid ephemeral port range.");
    if values.len() != 2 {
        return Err(invalid());
    }
    let first = values[0].parse::<u16>().map_err(|_| invalid())?;
    let last = values[1].parse::<u16>().map_err(|_| invalid())?;
    if first == 0 || first > last {
        return Err(invalid());
    }
    Ok(first..=last)
}

fn candidates(seed: u16, ephemeral: RangeInclusive<u16>) -> impl Iterator<Item = u16> {
    let lower_count = u32::from(*ephemeral.start()).saturating_sub(FIRST_PORT);
    let excluded_count = (u32::from(*ephemeral.end()) + 1).saturating_sub(FIRST_PORT + lower_count);
    let available_count = PORT_COUNT - excluded_count;
    (0..available_count).map(move |offset| {
        let index = (u32::from(seed) + offset) % available_count;
        (FIRST_PORT
            + index
            + if index >= lower_count {
                excluded_count
            } else {
                0
            }) as u16
    })
}

pub(super) async fn reserve() -> io::Result<(PortLease<'static>, Vec<TcpListener>)> {
    // Client connections can claim ephemeral source ports after the listeners are
    // released, before the child binds. The launch registry cannot prevent that.
    let ephemeral = ephemeral_range(
        &tokio::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range").await?,
    )?;
    let mut seed = [0; 2];
    getrandom::fill(&mut seed)
        .map_err(|_| io::Error::other("Kernel port selection randomness is unavailable."))?;
    reserve_from(
        &LEASED_PORTS,
        5,
        candidates(u16::from_ne_bytes(seed), ephemeral),
        |port| TcpListener::bind((Ipv4Addr::LOCALHOST, port)),
    )
    .await
}

async fn reserve_from<'registry, T, F, Fut>(
    registry: &'registry Mutex<BTreeSet<u16>>,
    count: usize,
    candidates: impl Iterator<Item = u16>,
    mut bind: F,
) -> io::Result<(PortLease<'registry>, Vec<T>)>
where
    F: FnMut(u16) -> Fut,
    Fut: Future<Output = io::Result<T>>,
{
    let mut lease = PortLease {
        registry,
        ports: Vec::with_capacity(count),
    };
    let mut listeners = Vec::new();
    for port in candidates.take(MAX_CANDIDATES) {
        // Even briefly binding a pending child's port could make its startup fail.
        let claimed = registry
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(port);
        if !claimed {
            continue;
        }
        lease.ports.push(port);
        match bind(port).await {
            Ok(listener) => {
                listeners.push(listener);
                if listeners.len() == count {
                    return Ok((lease, listeners));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => lease.release_last(),
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        "No kernel port set was available within the candidate limit.",
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::future::{poll_fn, ready};
    use std::task::Poll;

    use super::*;

    #[test]
    fn candidates_exclude_the_configured_ephemeral_range_before_limiting_attempts() {
        for range in [32_768..=60_999, 1_024..=50_000, 50_000..=65_535] {
            let choices: Vec<_> = candidates(40_000, range.clone()).collect();
            let unique: BTreeSet<_> = choices.iter().copied().collect();
            assert_eq!(choices.len(), unique.len());
            assert_eq!(choices.len(), PORT_COUNT as usize - range.clone().count());
            assert!(choices.iter().all(|port| !range.contains(port)));
            assert_eq!(choices.iter().take(MAX_CANDIDATES).count(), MAX_CANDIDATES);
        }
    }

    #[test]
    fn seeds_inside_the_ephemeral_range_do_not_converge_on_its_upper_boundary() {
        let first: BTreeSet<_> = (32_768..=60_999)
            .map(|seed| candidates(seed, 32_768..=60_999).next().unwrap())
            .collect();
        assert_eq!(first.len(), (60_999 - 32_768 + 1) as usize);
        assert_eq!(candidates(0, 1_024..=65_535).next(), None);
    }

    #[test]
    fn ephemeral_range_requires_two_ordered_nonzero_ports() {
        assert_eq!(ephemeral_range("32768\t60999\n").unwrap(), 32_768..=60_999);
        for invalid in ["", "0 10", "20 10", "10", "10 20 30", "10 65536", "a b"] {
            assert_eq!(
                ephemeral_range(invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    fn bind_token(port: u16) -> std::future::Ready<io::Result<u16>> {
        ready(Ok(port))
    }

    #[tokio::test]
    async fn pending_launch_ports_are_not_reallocated_after_listener_release() {
        let registry = Mutex::new(BTreeSet::new());
        let independent_registry = Mutex::new(BTreeSet::new());
        let port = 40_000;
        let (first, listeners) = reserve_from(&registry, 1, std::iter::once(port), bind_token)
            .await
            .unwrap();
        drop(listeners);

        // Another test's registry may own the same token without changing this test's state.
        let (_independent, _) =
            reserve_from(&independent_registry, 1, std::iter::once(port), bind_token)
                .await
                .unwrap();
        let (second, _) = reserve_from(&registry, 1, [port, port + 1].into_iter(), |candidate| {
            assert_ne!(candidate, port, "A pending port must never be rebound");
            bind_token(candidate)
        })
        .await
        .unwrap();
        assert_eq!(second.ports(), &[port + 1]);

        drop(first);
        let (parallel, _) = reserve_from(&registry, 1, std::iter::once(port), bind_token)
            .await
            .unwrap();
        let error = reserve_from(&registry, 1, std::iter::once(port), bind_token)
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        drop(parallel);
        let (reused, _) = reserve_from(&registry, 1, std::iter::once(port), bind_token)
            .await
            .unwrap();
        assert_eq!(reused.ports(), &[port]);
    }

    #[tokio::test]
    async fn concurrent_launches_own_five_distinct_ports_each() {
        let (first, second) = tokio::try_join!(reserve(), reserve()).unwrap();
        let distinct: BTreeSet<_> = first.0.ports().iter().chain(second.0.ports()).collect();
        assert_eq!(first.0.ports().len(), 5);
        assert_eq!(second.0.ports().len(), 5);
        assert_eq!(distinct.len(), 10);
    }

    #[tokio::test]
    async fn occupied_os_ports_are_skipped() {
        let (protected, mut listeners) = reserve().await.unwrap();
        let occupied = protected.ports()[0];
        let available = protected.ports()[4];
        // Retain the global claim while the scoped allocator exercises the real bind.
        drop(listeners.pop().unwrap());
        let registry = Mutex::new(BTreeSet::new());
        let (lease, bound) =
            reserve_from(&registry, 1, [occupied, available].into_iter(), |port| {
                TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            })
            .await
            .unwrap();
        assert_eq!(lease.ports(), &[available]);
        assert_eq!(bound[0].local_addr().unwrap().port(), available);
        assert_eq!(listeners[0].local_addr().unwrap().port(), occupied);
        // The failed bind must release its logical claim even while the OS port stays occupied.
        let (released, _) = reserve_from(&registry, 1, std::iter::once(occupied), bind_token)
            .await
            .unwrap();
        assert_eq!(released.ports(), &[occupied]);
    }

    #[tokio::test]
    async fn allocation_failure_releases_partial_leases_without_retrying_the_error() {
        let registry = Mutex::new(BTreeSet::new());
        let ports = [41_000, 41_001];
        let mut results = VecDeque::from([
            Ok(ports[0]),
            Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        ]);
        let error = reserve_from(&registry, 2, ports.into_iter(), |_| {
            ready(results.pop_front().expect("must not retry errors"))
        })
        .await
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(results.is_empty());
        let (lease, _) = reserve_from(&registry, 2, ports.into_iter(), bind_token)
            .await
            .unwrap();
        assert_eq!(lease.ports(), &ports);
    }

    #[tokio::test]
    async fn allocation_cancellation_releases_partial_leases() {
        let registry = Mutex::new(BTreeSet::new());
        let ports = [42_000, 42_001];
        let mut partial = Box::pin(reserve_from(
            &registry,
            2,
            ports.into_iter(),
            |port| async move {
                if port == ports[0] {
                    Ok(port)
                } else {
                    std::future::pending::<io::Result<u16>>().await
                }
            },
        ));
        poll_fn(|context| {
            assert!(partial.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        let error = reserve_from(&registry, 1, ports.into_iter(), bind_token)
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        drop(partial);
        let (lease, _) = reserve_from(&registry, 2, ports.into_iter(), bind_token)
            .await
            .unwrap();
        assert_eq!(lease.ports(), &ports);
    }

    #[tokio::test]
    async fn occupied_candidates_exhaust_the_bounded_search_without_binding() {
        let registry = Mutex::new(BTreeSet::new());
        let (first, listeners) = reserve_from(&registry, 1, std::iter::once(43_000), bind_token)
            .await
            .unwrap();
        drop(listeners);
        let mut attempts = 0;
        let choices = std::iter::repeat(first.ports()[0]).inspect(|_| attempts += 1);
        let error = reserve_from(
            &registry,
            1,
            choices,
            |_| -> std::future::Ready<io::Result<u16>> {
                panic!("An owned port must never be rebound");
            },
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert_eq!(attempts, MAX_CANDIDATES);
    }
}
