use serde::Serialize;
use tokio::task::{JoinSet, JoinError};

use crate::{HarmonicError, InstallSettings};

use crate::actions::base::{CreateGroup, CreateGroupError, CreateUserError};
use crate::actions::{ActionDescription, Actionable, CreateUser, ActionState, Action};

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone)]
pub struct CreateUsersAndGroup {
    daemon_user_count: usize,
    nix_build_group_name: String,
    nix_build_group_id: usize,
    nix_build_user_prefix: String,
    nix_build_user_id_base: usize,
    create_group: CreateGroup,
    create_users: Vec<CreateUser>,
    action_state: ActionState,
}

impl CreateUsersAndGroup {
    #[tracing::instrument(skip_all)]
    pub async fn plan(settings: InstallSettings) -> Result<Self, CreateUsersAndGroupError> {
        // TODO(@hoverbear): CHeck if it exist, error if so
        let create_group = CreateGroup::plan(
            settings.nix_build_group_name.clone(),
            settings.nix_build_group_id,
        );
        // TODO(@hoverbear): CHeck if they exist, error if so
        let create_users = (0..settings.daemon_user_count)
            .map(|count| {
                CreateUser::plan(
                    format!("{}{count}", settings.nix_build_user_prefix),
                    settings.nix_build_user_id_base + count,
                    settings.nix_build_group_id,
                )
            })
            .collect();
        Ok(Self {
            daemon_user_count: settings.daemon_user_count,
            nix_build_group_name: settings.nix_build_group_name,
            nix_build_group_id: settings.nix_build_group_id,
            nix_build_user_prefix: settings.nix_build_user_prefix,
            nix_build_user_id_base: settings.nix_build_user_id_base,
            create_group,
            create_users,
            action_state: ActionState::Planned,
        })
    }
}

#[async_trait::async_trait]
impl Actionable for CreateUsersAndGroup {
    type Error = CreateUsersAndGroupError;
    fn description(&self) -> Vec<ActionDescription> {
        let Self {
            daemon_user_count,
            nix_build_group_name,
            nix_build_group_id,
            nix_build_user_prefix,
            nix_build_user_id_base,
            create_group: _,
            create_users: _,
            action_state: _,
        } = &self;

        vec![
            ActionDescription::new(
                format!("Create build users and group"),
                vec![
                    format!("The nix daemon requires system users (and a group they share) which it can act as in order to build"),
                    format!("Create group `{nix_build_group_name}` with uid `{nix_build_group_id}`"),
                    format!("Create {daemon_user_count} users with prefix `{nix_build_user_prefix}` starting at uid `{nix_build_user_id_base}`"),
                ],
            )
        ]
    }

    #[tracing::instrument(skip_all)]
    async fn execute(&mut self) -> Result<(), Self::Error> {
        let Self {
            create_users,
            create_group, 
            daemon_user_count: _, 
            nix_build_group_name: _,
            nix_build_group_id: _, 
            nix_build_user_prefix: _, 
            nix_build_user_id_base: _, 
            action_state,
        } = self;


        // Create group
        create_group.execute().await?;

        // Create users
        // TODO(@hoverbear): Abstract this, it will be common
        let mut set = JoinSet::new();

        let mut errors = Vec::default();

        for (idx, create_user) in create_users.iter().enumerate() {
            let mut create_user_clone = create_user.clone();
            let _abort_handle = set.spawn(async move { create_user_clone.execute().await?; Result::<_, CreateUserError>::Ok((idx, create_user_clone)) });
        }

        while let Some(result) = set.join_next().await {
            match result {
                Ok(Ok((idx, success))) => create_users[idx] = success,
                Ok(Err(e)) => errors.push(e),
                Err(e) => return Err(e)?,
            };
        }

        if !errors.is_empty() {
            if errors.len() == 1 {
                return Err(errors.into_iter().next().unwrap().into());
            } else {
                return Err(CreateUsersAndGroupError::CreateUsers(errors));
            }
        }


        *action_state = ActionState::Completed;
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    async fn revert(&mut self) -> Result<(), Self::Error> {
        let Self {
            create_users,
            create_group, 
            daemon_user_count: _, 
            nix_build_group_name: _,
            nix_build_group_id: _, 
            nix_build_user_prefix: _, 
            nix_build_user_id_base: _, 
            action_state,
        } = self;

        // Create users
        // TODO(@hoverbear): Abstract this, it will be common
        let mut set = JoinSet::new();

        let mut errors = Vec::default();

        for (idx, create_user) in create_users.iter().enumerate() {
            let mut create_user_clone = create_user.clone();
            let _abort_handle = set.spawn(async move { create_user_clone.revert().await?; Result::<_, CreateUserError>::Ok((idx, create_user_clone)) });
        }

        while let Some(result) = set.join_next().await {
            match result {
                Ok(Ok((idx, success))) => create_users[idx] = success,
                Ok(Err(e)) => errors.push(e),
                Err(e) => return Err(e)?,
            };
        }

        if !errors.is_empty() {
            if errors.len() == 1 {
                return Err(errors.into_iter().next().unwrap().into());
            } else {
                return Err(CreateUsersAndGroupError::CreateUsers(errors));
            }
        }
        
        // Create group
        create_group.revert().await?;

        *action_state = ActionState::Reverted;
        Ok(())
    }
}


impl From<CreateUsersAndGroup> for Action {
    fn from(v: CreateUsersAndGroup) -> Self {
        Action::CreateUsersAndGroup(v)
    }
}


#[derive(Debug, thiserror::Error, Serialize)]
pub enum CreateUsersAndGroupError {
    #[error(transparent)]
    CreateUser(#[from] CreateUserError),
    #[error("Multiple errors: {}", .0.iter().map(|v| format!("{v}")).collect::<Vec<_>>().join(" & "))]
    CreateUsers(Vec<CreateUserError>),
    #[error(transparent)]
    CreateGroup(#[from] CreateGroupError),
    #[error(transparent)]
    Join(
        #[from]
        #[serde(serialize_with = "crate::serialize_error_to_display")]
        JoinError
    ),
}

