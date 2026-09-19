//! Consumer-facing acceptance for Sim;X's UI integration requests.
mod sim_x {
    mod controls;
    mod focus;
    mod load;
    #[cfg(feature = "headless-text")]
    mod text;
    mod visuals;
}
