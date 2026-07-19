//! `sidebar-sensor` — SensorProvider trait + cost classifier (keystone, AD-4/AD-5).
//!
//! The `SensorProvider` trait is the single contract every adapter implements.
//! The `classify_for_v1` gate filters providers by cost class (NFR-1) and
//! tier before they enter the v1 registry.

pub mod classifier;
pub mod descriptor;
pub mod provider;
