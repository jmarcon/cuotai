//! PRD 0048 RF3.4: the consoles a capture names (`CaptureSpec::consoles`).

use super::*;

/// A capture names only its own descriptor's hosts as consoles, and its
/// window can always load the address whose cookies it reads.
#[test]
fn capture_consoles_are_descriptor_hosts_and_cover_the_cookie_host() {
    for (provider, slot, region, spec) in every_capture() {
        let d = descriptors(provider)
            .iter()
            .find(|d| d.slot == slot)
            .expect("the capture's descriptor");
        for console in spec.consoles {
            assert!(
                d.hosts.contains(console),
                "{provider:?} {slot} {region:?}: {console} is not a descriptor host"
            );
        }
        assert!(
            d.capture_allows_with(spec, spec.cookie_url),
            "{provider:?} {slot} {region:?}: the cookie host is not allowed"
        );
        assert!(
            d.capture_allows_in(region, spec.cookie_url),
            "{provider:?} {slot} {region:?}: the cookie host is not allowed in its region"
        );
    }
}
