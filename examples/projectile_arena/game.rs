use sim_logic::prelude::*;

const PLAYER_SPEED: f32 = 10.0;
const PROJECTILE_RADIUS: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerAction {
    Up,
    Down,
    Left,
    Right,
    Fire,
}

const MOVEMENT: DigitalAxis2d<PlayerAction> = DigitalAxis2d::new(
    PlayerAction::Left,
    PlayerAction::Right,
    PlayerAction::Down,
    PlayerAction::Up,
);

#[derive(Component)]
pub struct Player;

#[derive(Component, Clone, Copy)]
pub struct Projectile;

#[derive(Component)]
#[require(ClaimedTarget)]
pub struct Target;

#[derive(Component, Default)]
struct ClaimedTarget(bool);

#[derive(Component, Clone, Copy)]
pub struct LifetimeTicks(u16);

#[derive(Resource, Default)]
pub struct Score(pub u32);

#[derive(Resource, Clone, Copy)]
struct ProjectileSpec {
    velocity: LinearVelocity2d,
    lifetime_ticks: u16,
    visual: CircleVisual,
    collider: CircleCollider2d,
}

type ProjectileData = (
    LogicEntityRef,
    &'static Transform2d,
    &'static CircleCollider2d,
    &'static mut LifetimeTicks,
);
type ProjectileFilter = (With<Projectile>, Without<Target>);
type TargetFilter = (With<Target>, Without<Projectile>);

fn fire_projectile(
    input: FixedInput<PlayerAction>,
    player: Single<&Transform2d, With<Player>>,
    spec: Res<ProjectileSpec>,
    mut commands: Commands,
) -> LogicResult {
    let shots = input.pressed(PlayerAction::Fire).count();
    if shots == 0 {
        return Ok(());
    }
    let transform = Transform2d::new(player.translation())?;
    commands.spawn_copies(
        (
            Projectile,
            transform,
            spec.visual,
            spec.collider,
            spec.velocity,
            LifetimeTicks(spec.lifetime_ticks),
        ),
        shots,
    )?;
    Ok(())
}

fn resolve_projectiles(
    mut projectiles: Query<ProjectileData, ProjectileFilter>,
    targets: CircleOverlapEntities<TargetFilter>,
    mut target_claims: Query<&mut ClaimedTarget>,
    mut score: ResMut<Score>,
    mut commands: Commands,
) -> LogicResult {
    for (projectile_entity, projectile_transform, projectile_collider, mut lifetime) in
        &mut projectiles
    {
        let next_lifetime = lifetime.0.saturating_sub(1);
        let expired = next_lifetime == 0;

        let mut hit = None;
        for target in targets.iter_overlapping(
            projectile_entity.handle(),
            projectile_transform,
            projectile_collider,
        )? {
            let claim = target_claims.get(target)?;
            if claim.0 {
                continue;
            }
            // Commands are deferred, so later projectiles can still observe
            // this target in the overlap query. Record it now, but do not claim
            // it until both commands for this hit have enqueued successfully.
            hit = Some(target);
            break;
        }

        // If several unclaimed targets overlap one projectile, the first one
        // follows ECS query order. A game that needs a stable gameplay
        // tie-breaker must select it explicitly.
        if let Some(target) = hit {
            commands.despawn(target)?;
            commands.despawn(projectile_entity.handle())?;
            let mut claim = target_claims.get_mut(target)?;
            debug_assert!(!claim.0, "selected target should remain unclaimed");
            claim.0 = true;
            // Ordinary component/Resource writes are immediate, while
            // Commands commit at the stage barrier. A later System error or
            // barrier rejection could discard them without rolling back
            // the claim or Score; this small example treats either as terminal.
            score.0 = score.0.saturating_add(1);
            continue;
        }

        if expired {
            commands.despawn(projectile_entity.handle())?;
        }
        // An expiring projectile reaches this write only after its own despawn
        // has enqueued. A later failure still follows non-transactional stage
        // semantics and is terminal for this example.
        lifetime.0 = next_lifetime;
    }
    Ok(())
}

pub(crate) fn build_application(
    time: TimeConfig,
) -> LogicResult<(Application<PlayerAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    let mut application = Application::new(config)?;

    application
        .approve_components::<(Player, Projectile, Target, ClaimedTarget, LifetimeTicks)>()?;

    application.bind_wasd_and_arrows(MOVEMENT)?;
    application.bind_key(PhysicalKeyCode::Space, PlayerAction::Fire)?;

    let camera = ActiveCamera2d::centered(20.0)?;
    let player_visual = CircleVisual::new(0.6, Color::rgb8(68, 144, 255))?;
    let player_movement = DigitalMovement2d::new(MOVEMENT, PLAYER_SPEED)?;
    let (target_visual, target_collider) =
        CircleVisual::new(0.4, Color::rgb8(255, 92, 92))?.with_matching_collider();
    let target_transform = Transform2d::from_xy(3.0, 0.0)?;
    let (projectile_visual, projectile_collider) =
        CircleVisual::new(PROJECTILE_RADIUS, Color::rgb8(255, 220, 64))?.with_matching_collider();
    let projectile_spec = ProjectileSpec {
        velocity: LinearVelocity2d::new(Vec2::new(10.0, 0.0))?,
        lifetime_ticks: 180,
        visual: projectile_visual,
        collider: projectile_collider,
    };
    let initial_world = application.register_world("projectile-arena", move |world| {
        world.spawn(camera)?;
        world.spawn((Player, player_visual, player_movement))?;
        world.spawn((Target, target_transform, target_visual, target_collider))?;
        world.insert_resource(Score::default())?;
        world.insert_resource(projectile_spec)?;
        Ok(())
    })?;

    // Systems run in this order. A projectile queued by `fire_projectile`
    // becomes visible only after the complete FixedUpdate stage, so it starts
    // moving on the next fixed tick rather than halfway through this one.
    application.add_digital_movement2d_system();
    application.add_fallible_fixed_system(fire_projectile);
    application.add_linear_velocity2d_system();
    application.add_fallible_fixed_system(resolve_projectiles);

    Ok((application, initial_world))
}

#[cfg(test)]
#[allow(
    dead_code,
    reason = "used by the integration target that includes this shared example module"
)]
pub(crate) fn build_multi_hit_regression(
    time: TimeConfig,
) -> LogicResult<(Application<PlayerAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    let mut application = Application::new(config)?;
    application.approve_components::<(Projectile, Target, ClaimedTarget, LifetimeTicks)>()?;
    application.add_fallible_fixed_system(resolve_projectiles);

    let camera = ActiveCamera2d::centered(20.0)?;
    let collider = CircleCollider2d::new(PROJECTILE_RADIUS)?;
    let left = Transform2d::from_xy(-4.0, 0.0)?;
    let right = Transform2d::from_xy(4.0, 0.0)?;
    let world = application.register_world("multi-hit-regression", move |world| {
        world.spawn(camera)?;
        // Both expiring projectiles overlap one target. Whichever is visited
        // first claims it; the other must still reach its expiry branch.
        world.spawn((Projectile, left, collider, LifetimeTicks(1)))?;
        world.spawn((Projectile, left, collider, LifetimeTicks(1)))?;
        world.spawn((Projectile, right, collider, LifetimeTicks(5)))?;
        // Required-component insertion supplies a fresh claim automatically.
        world.spawn((Target, left, collider))?;
        world.spawn((Target, right, collider))?;
        world.insert_resource(Score::default())?;
        Ok(())
    })?;
    Ok((application, world))
}

#[cfg(test)]
#[allow(
    dead_code,
    reason = "used by the integration target that includes this shared example module"
)]
pub(crate) fn build_enqueue_failure_regression(
    time: TimeConfig,
) -> LogicResult<(Application<PlayerAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    config.set_command_limit(1)?;
    let mut application = Application::new(config)?;
    application.approve_components::<(Projectile, Target, ClaimedTarget, LifetimeTicks)>()?;
    application.add_fallible_fixed_system(resolve_projectiles);

    let camera = ActiveCamera2d::centered(20.0)?;
    let collider = CircleCollider2d::new(PROJECTILE_RADIUS)?;
    let transform = Transform2d::default();
    let world = application.register_world("enqueue-failure-regression", move |world| {
        world.spawn(camera)?;
        world.spawn((Projectile, transform, collider, LifetimeTicks(2)))?;
        world.spawn((Target, transform, collider))?;
        world.insert_resource(Score::default())?;
        Ok(())
    })?;
    Ok((application, world))
}

#[cfg(test)]
#[allow(
    dead_code,
    reason = "used by the integration target that includes this shared example module"
)]
pub(crate) fn enqueue_failure_state(
    runner: &HeadlessRunner<PlayerAction>,
) -> Option<(usize, usize, u32, bool, u16)> {
    let projectile_count = runner.components::<Projectile>().count();
    let target_count = runner.components::<Target>().count();
    let score = runner.resource::<Score>()?.0;
    let claimed = runner.components::<ClaimedTarget>().next()?.1.0;
    let lifetime = runner.components::<LifetimeTicks>().next()?.1.0;
    Some((projectile_count, target_count, score, claimed, lifetime))
}
