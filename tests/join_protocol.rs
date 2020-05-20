// Copyright 2020 nytopop (Eric Izoita)
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.
mod shared;

use blip::Mesh;
use futures::future::{join, join3, FutureExt};
use shared::CfgHandle;
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicU8, Ordering::Relaxed},
};
use tokio::select;

// A quick NOTE about addressing in tests
//
//      +----------- always 127
//     /   +-------- unique per module
//    /   /   +----- unique per test fn { subnet() }
//   /   /   /   /-- unique per node
//  v   v   v   v
// 127 254 254 254 : 10000 (port always 10000)
//
// this lets us bind lots of sockets without encountering address collisions

fn subnet() -> u8 {
    static SUBNET: AtomicU8 = AtomicU8::new(1);
    let s = SUBNET.fetch_add(1, Relaxed);
    assert!(s != 255 && s != 0);
    s
}

fn addr_in(subnet: u8, host: u8) -> SocketAddr {
    ([127, 0, subnet, host], 10000).into()
}

/// Tests that a single node can bootstrap a configuration without any other nodes.
#[tokio::test]
async fn single_node_cluster_bootstrap() {
    let h = CfgHandle::default();

    let srv = Mesh::default()
        .add_mesh_service(h.clone())
        .serve(addr_in(subnet(), 1));

    select! {
        e = srv => panic!("mesh exited with: {:?}", e),
        _ = h.cfg_change(1) => {}
    }
}

/// Tests that three nodes can converge on a single configuration that includes all of them.
#[tokio::test]
async fn three_node_cluster_bootstrap() {
    let net = subnet();

    let h1 = CfgHandle::default();
    let s1 = Mesh::default()
        .add_mesh_service(h1.clone())
        .serve(addr_in(net, 1));

    let h2 = CfgHandle::default();
    let s2 = Mesh::default()
        .add_mesh_service(h2.clone())
        .join_seed(addr_in(net, 1), false)
        .serve(addr_in(net, 2));

    let h3 = CfgHandle::default();
    let s3 = Mesh::default()
        .add_mesh_service(h3.clone())
        .join_seed(addr_in(net, 1), false)
        .serve(addr_in(net, 3));

    select! {
        e = s1 => panic!("s1 exited with: {:?}", e),
        e = s2 => panic!("s2 exited with: {:?}", e),
        e = s3 => panic!("s3 exited with: {:?}", e),

        (c1, c2, c3) = join3(h1.cfg_change(3), h2.cfg_change(3), h3.cfg_change(3)) => {
            assert!(c1.conf_id() == c2.conf_id());
            assert!(c2.conf_id() == c3.conf_id());
        }
    }
}

/// Tests that in the event a member of a three node configuration becomes partitioned from
/// the others, it is ejected from the configuration. Once it comes back online, it should
/// rejoin the mesh.
#[tokio::test]
async fn three_node_cluster_partition_recovery() {
    let net = subnet();

    let h1 = CfgHandle::default();
    let mut s1 = Mesh::default()
        .add_mesh_service(h1.clone())
        .serve(addr_in(net, 1))
        .boxed();

    let h2 = CfgHandle::default();
    let mut s2 = Mesh::default()
        .add_mesh_service(h2.clone())
        .join_seed(addr_in(net, 1), false)
        .serve(addr_in(net, 2))
        .boxed();

    let h3 = CfgHandle::default();
    let mut s3 = Mesh::default()
        .add_mesh_service(h3.clone())
        .join_seed(addr_in(net, 1), false)
        .serve(addr_in(net, 3))
        .boxed();

    // wait for cluster to bootstrap
    select! {
        e = &mut s1 => panic!("s1 exited with: {:?}", e),
        e = &mut s2 => panic!("s2 exited with: {:?}", e),
        e = &mut s3 => panic!("s3 exited with: {:?}", e),

        (c1, c2, c3) = join3(h1.cfg_change(3), h2.cfg_change(3), h3.cfg_change(3)) => {
            assert!(c1.conf_id() == c2.conf_id());
            assert!(c2.conf_id() == c3.conf_id());
        }
    }

    // progress s1/s2 but not s3 and wait for ejection
    select! {
        e = &mut s1 => panic!("s1 exited with: {:?}", e),
        e = &mut s2 => panic!("s2 exited with: {:?}", e),

        (c1, c2) = join(h1.cfg_change(2), h2.cfg_change(2)) => {
            assert!(c1.conf_id() == c2.conf_id());
        }
    }

    // wait for cluster to re-converge
    select! {
        e = &mut s1 => panic!("s1 exited with: {:?}", e),
        e = &mut s2 => panic!("s2 exited with: {:?}", e),
        e = &mut s3 => panic!("s3 exited with: {:?}", e),

        (c1, c2, c3) = join3(h1.cfg_change(3), h2.cfg_change(3), h3.cfg_change(3)) => {
            assert!(c1.conf_id() == c2.conf_id());
            assert!(c2.conf_id() == c3.conf_id());
        }
    }
}
