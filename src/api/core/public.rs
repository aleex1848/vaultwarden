use chrono::Utc;
use rocket::{
    form::FromForm,
    request::{FromRequest, Outcome},
    serde::json::Json,
    Request, Route,
};

use std::collections::{HashMap, HashSet};

use crate::{
    api::{core::log_event, EmptyResult, JsonResult},
    auth,
    db::{
        models::{
            Collection, CollectionGroup, CollectionId, CollectionUser, EventType, Group, GroupId, GroupUser, Invitation,
            Membership, MembershipId, MembershipStatus, MembershipType, Organization, OrganizationApiKey,
            OrganizationId, User, UserId,
        },
        DbConn,
    },
    mail,
    util::NumberOrString,
    CONFIG,
};

pub fn routes() -> Vec<Route> {
    routes![
        ldap_import,
        public_get_groups,
        public_get_groups_details,
        public_get_group,
        public_get_group_details,
        public_post_group,
        public_put_group,
        public_delete_group,
        public_get_group_members,
        public_put_group_members,
        public_get_members,
        public_get_member,
        public_post_members_invite,
        public_put_member,
        public_delete_member,
        public_get_collections,
        public_get_collection,
        public_post_collection,
        public_put_collection,
        public_delete_collection,
    ]
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrgImportGroupData {
    name: String,
    external_id: String,
    member_external_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrgImportUserData {
    email: String,
    external_id: String,
    deleted: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrgImportData {
    groups: Vec<OrgImportGroupData>,
    members: Vec<OrgImportUserData>,
    overwrite_existing: bool,
    // largeImport: bool, // For now this will not be used, upstream uses this to prevent syncs of more then 2000 users or groups without the flag set.
}

#[post("/public/organization/import", data = "<data>")]
async fn ldap_import(data: Json<OrgImportData>, token: PublicToken, conn: DbConn) -> EmptyResult {
    // Most of the logic for this function can be found here
    // https://github.com/bitwarden/server/blob/9ebe16587175b1c0e9208f84397bb75d0d595510/src/Core/AdminConsole/Services/Implementations/OrganizationService.cs#L1203

    let org_id = token.0;
    let data = data.into_inner();

    for user_data in &data.members {
        let mut user_created: bool = false;
        if user_data.deleted {
            // If user is marked for deletion and it exists, revoke it
            if let Some(mut member) = Membership::find_by_email_and_org(&user_data.email, &org_id, &conn).await {
                // Only revoke a user if it is not the last confirmed owner
                let revoked = if member.atype == MembershipType::Owner
                    && member.status == MembershipStatus::Confirmed as i32
                {
                    if Membership::count_confirmed_by_org_and_type(&org_id, MembershipType::Owner, &conn).await <= 1 {
                        warn!("Can't revoke the last owner");
                        false
                    } else {
                        member.revoke()
                    }
                } else {
                    member.revoke()
                };

                let ext_modified = member.set_external_id(Some(user_data.external_id.clone()));
                if revoked || ext_modified {
                    member.save(&conn).await?;
                }
            }
        // If user is part of the organization, restore it
        } else if let Some(mut member) = Membership::find_by_email_and_org(&user_data.email, &org_id, &conn).await {
            let restored = member.restore();
            let ext_modified = member.set_external_id(Some(user_data.external_id.clone()));
            if restored || ext_modified {
                member.save(&conn).await?;
            }
        } else {
            // If user is not part of the organization
            let user = match User::find_by_mail(&user_data.email, &conn).await {
                Some(user) => user, // exists in vaultwarden
                None => {
                    // User does not exist yet
                    let mut new_user = User::new(&user_data.email, None);
                    new_user.save(&conn).await?;

                    if !CONFIG.mail_enabled() {
                        Invitation::new(&new_user.email).save(&conn).await?;
                    }
                    user_created = true;
                    new_user
                }
            };
            let member_status = if CONFIG.mail_enabled() || user.password_hash.is_empty() {
                MembershipStatus::Invited as i32
            } else {
                MembershipStatus::Accepted as i32 // Automatically mark user as accepted if no email invites
            };

            let (org_name, org_email) = match Organization::find_by_uuid(&org_id, &conn).await {
                Some(org) => (org.name, org.billing_email),
                None => err!("Error looking up organization"),
            };

            let mut new_member = Membership::new(user.uuid.clone(), org_id.clone(), Some(org_email.clone()));
            new_member.set_external_id(Some(user_data.external_id.clone()));
            new_member.access_all = false;
            new_member.atype = MembershipType::User as i32;
            new_member.status = member_status;

            new_member.save(&conn).await?;

            if CONFIG.mail_enabled() {
                if let Err(e) =
                    mail::send_invite(&user, org_id.clone(), new_member.uuid.clone(), &org_name, Some(org_email)).await
                {
                    // Upon error delete the user, invite and org member records when needed
                    if user_created {
                        user.delete(&conn).await?;
                    } else {
                        new_member.delete(&conn).await?;
                    }

                    err!(format!("Error sending invite: {e:?} "));
                }
            }
        }
    }

    if CONFIG.org_groups_enabled() {
        for group_data in &data.groups {
            let group_uuid = match Group::find_by_external_id_and_org(&group_data.external_id, &org_id, &conn).await {
                Some(group) => group.uuid,
                None => {
                    let mut group = Group::new(
                        org_id.clone(),
                        group_data.name.clone(),
                        false,
                        Some(group_data.external_id.clone()),
                    );
                    group.save(&conn).await?;
                    group.uuid
                }
            };

            GroupUser::delete_all_by_group(&group_uuid, &conn).await?;

            for ext_id in &group_data.member_external_ids {
                if let Some(member) = Membership::find_by_external_id_and_org(ext_id, &org_id, &conn).await {
                    let mut group_user = GroupUser::new(group_uuid.clone(), member.uuid.clone());
                    group_user.save(&conn).await?;
                }
            }
        }
    } else {
        warn!("Group support is disabled, groups will not be imported!");
    }

    // If this flag is enabled, any user that isn't provided in the Users list will be removed (by default they will be kept unless they have Deleted == true)
    if data.overwrite_existing {
        // Generate a HashSet to quickly verify if a member is listed or not.
        let sync_members: HashSet<String> = data.members.into_iter().map(|m| m.external_id).collect();
        for member in Membership::find_by_org(&org_id, &conn).await {
            if let Some(ref user_external_id) = member.external_id {
                if !sync_members.contains(user_external_id) {
                    if member.atype == MembershipType::Owner && member.status == MembershipStatus::Confirmed as i32 {
                        // Removing owner, check that there is at least one other confirmed owner
                        if Membership::count_confirmed_by_org_and_type(&org_id, MembershipType::Owner, &conn).await <= 1
                        {
                            warn!("Can't delete the last owner");
                            continue;
                        }
                    }
                    member.delete(&conn).await?;
                }
            }
        }
    }

    Ok(())
}

// Structs for Public API requests
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicGroupRequest {
    name: String,
    #[serde(default)]
    access_all: bool,
    external_id: Option<String>,
    collections: Vec<PublicCollectionData>,
    users: Vec<MembershipId>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicCollectionData {
    id: CollectionId,
    read_only: bool,
    hide_passwords: bool,
    manage: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicInviteData {
    emails: Vec<String>,
    groups: Vec<GroupId>,
    r#type: NumberOrString,
    collections: Option<Vec<PublicCollectionData>>,
    #[serde(default)]
    permissions: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicEditUserData {
    r#type: NumberOrString,
    collections: Option<Vec<PublicCollectionData>>,
    groups: Option<Vec<GroupId>>,
    #[serde(default)]
    permissions: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicCollectionRequest {
    name: String,
    external_id: Option<String>,
    groups: Vec<PublicCollectionGroupData>,
    users: Vec<PublicCollectionMembershipData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicCollectionGroupData {
    id: GroupId,
    read_only: bool,
    hide_passwords: bool,
    manage: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicCollectionMembershipData {
    id: MembershipId,
    read_only: bool,
    hide_passwords: bool,
    manage: bool,
}

#[derive(FromForm)]
struct PublicGetOrgUserData {
    #[field(name = "includeCollections")]
    include_collections: Option<bool>,
    #[field(name = "includeGroups")]
    include_groups: Option<bool>,
}

pub struct PublicToken(OrganizationId);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for PublicToken {
    type Error = &'static str;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let headers = request.headers();
        // Get access_token
        let access_token: &str = match headers.get_one("Authorization") {
            Some(a) => match a.rsplit("Bearer ").next() {
                Some(split) => split,
                None => err_handler!("No access token provided"),
            },
            None => err_handler!("No access token provided"),
        };
        // Check JWT token is valid and get device and user from it
        let Ok(claims) = auth::decode_api_org(access_token) else {
            err_handler!("Invalid claim")
        };
        // Check if time is between claims.nbf and claims.exp
        let time_now = Utc::now().timestamp();
        if time_now < claims.nbf {
            err_handler!("Token issued in the future");
        }
        if time_now > claims.exp {
            err_handler!("Token expired");
        }
        // Check if claims.iss is domain|claims.scope[0]
        let complete_host = format!("{}|{}", CONFIG.domain_origin(), claims.scope[0]);
        if complete_host != claims.iss {
            err_handler!("Token not issued by this server");
        }

        // Check if claims.sub is org_api_key.uuid
        // Check if claims.client_sub is org_api_key.org_uuid
        let conn = match DbConn::from_request(request).await {
            Outcome::Success(conn) => conn,
            _ => err_handler!("Error getting DB"),
        };
        let Some(org_id) = claims.client_id.strip_prefix("organization.") else {
            err_handler!("Malformed client_id")
        };
        let org_id: OrganizationId = org_id.to_string().into();
        let Some(org_api_key) = OrganizationApiKey::find_by_org_uuid(&org_id, &conn).await else {
            err_handler!("Invalid client_id")
        };
        if org_api_key.org_uuid != claims.client_sub {
            err_handler!("Token not issued for this org");
        }
        if org_api_key.uuid != claims.sub {
            err_handler!("Token not issued for this client");
        }

        Outcome::Success(PublicToken(claims.client_sub))
    }
}

// Helper functions for Public API
async fn public_get_groups_data(details: bool, org_id: OrganizationId, conn: &DbConn) -> JsonResult {
    let groups: Vec<serde_json::Value> = if CONFIG.org_groups_enabled() {
        let groups = Group::find_by_organization(&org_id, conn).await;
        let mut groups_json = Vec::with_capacity(groups.len());

        if details {
            for g in groups {
                groups_json.push(g.to_json_details(conn).await)
            }
        } else {
            for g in groups {
                groups_json.push(g.to_json())
            }
        }
        groups_json
    } else {
        Vec::with_capacity(0)
    };

    Ok(Json(serde_json::json!({
        "data": groups,
        "object": "list",
        "continuationToken": null,
    })))
}

async fn public_add_update_group(
    mut group: Group,
    collections: Vec<PublicCollectionData>,
    members: Vec<MembershipId>,
    _org_id: OrganizationId,
    conn: &DbConn,
) -> JsonResult {
    group.save(conn).await?;

    for col_selection in collections {
        let mut collection_group = CollectionGroup::new(
            col_selection.id.clone(),
            group.uuid.clone(),
            col_selection.read_only,
            col_selection.hide_passwords,
            col_selection.manage,
        );
        collection_group.save(conn).await?;
    }

    for assigned_member in members {
        let mut user_entry = GroupUser::new(group.uuid.clone(), assigned_member.clone());
        user_entry.save(conn).await?;
    }

    Ok(Json(serde_json::json!({
        "id": group.uuid,
        "organizationId": group.organizations_uuid,
        "name": group.name,
        "accessAll": group.access_all,
        "externalId": group.external_id,
        "object": "group"
    })))
}

// Public API Group Endpoints
#[get("/public/groups")]
async fn public_get_groups(token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    public_get_groups_data(false, org_id, &conn).await
}

#[get("/public/groups/details")]
async fn public_get_groups_details(token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    public_get_groups_data(true, org_id, &conn).await
}

#[get("/public/groups/<group_id>")]
async fn public_get_group(group_id: GroupId, token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    if !CONFIG.org_groups_enabled() {
        err!("Group support is disabled");
    }

    let Some(group) = Group::find_by_uuid_and_org(&group_id, &org_id, &conn).await else {
        err!("Group not found", "Group uuid is invalid or does not belong to the organization")
    };

    Ok(Json(group.to_json()))
}

#[get("/public/groups/<group_id>/details")]
async fn public_get_group_details(group_id: GroupId, token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    if !CONFIG.org_groups_enabled() {
        err!("Group support is disabled");
    }

    let Some(group) = Group::find_by_uuid_and_org(&group_id, &org_id, &conn).await else {
        err!("Group not found", "Group uuid is invalid or does not belong to the organization")
    };

    Ok(Json(group.to_json_details(&conn).await))
}

#[post("/public/groups", data = "<data>")]
async fn public_post_group(data: Json<PublicGroupRequest>, token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    if !CONFIG.org_groups_enabled() {
        err!("Group support is disabled");
    }

    let group_request = data.into_inner();
    let group = Group::new(org_id.clone(), group_request.name.clone(), group_request.access_all, group_request.external_id.clone());

    // Note: Event logging without user context - using empty UserId for API calls
    log_event(
        EventType::GroupCreated as i32,
        &group.uuid,
        &org_id,
        &UserId::from("00000000-0000-0000-0000-000000000000"),
        0, // Device type 0 = API
        &std::net::IpAddr::from([0, 0, 0, 0]),
        &conn,
    )
    .await;

    public_add_update_group(group, group_request.collections, group_request.users, org_id, &conn).await
}

#[put("/public/groups/<group_id>", data = "<data>")]
async fn public_put_group(
    group_id: GroupId,
    data: Json<PublicGroupRequest>,
    token: PublicToken,
    conn: DbConn,
) -> JsonResult {
    let org_id = token.0;
    if !CONFIG.org_groups_enabled() {
        err!("Group support is disabled");
    }

    let Some(mut group) = Group::find_by_uuid_and_org(&group_id, &org_id, &conn).await else {
        err!("Group not found", "Group uuid is invalid or does not belong to the organization")
    };

    let group_request = data.into_inner();
    group.name.clone_from(&group_request.name);
    group.access_all = group_request.access_all;

    CollectionGroup::delete_all_by_group(&group_id, &conn).await?;
    GroupUser::delete_all_by_group(&group_id, &conn).await?;

    // Note: Event logging without user context
    log_event(
        EventType::GroupUpdated as i32,
        &group.uuid,
        &org_id,
        &UserId::from("00000000-0000-0000-0000-000000000000"),
        0,
        &std::net::IpAddr::from([0, 0, 0, 0]),
        &conn,
    )
    .await;

    public_add_update_group(group, group_request.collections, group_request.users, org_id, &conn).await
}

#[delete("/public/groups/<group_id>")]
async fn public_delete_group(group_id: GroupId, token: PublicToken, conn: DbConn) -> EmptyResult {
    let org_id = token.0;
    if !CONFIG.org_groups_enabled() {
        err!("Group support is disabled");
    }

    let Some(group) = Group::find_by_uuid_and_org(&group_id, &org_id, &conn).await else {
        err!("Group not found", "Group uuid is invalid or does not belong to the organization")
    };

    // Note: Event logging without user context
    log_event(
        EventType::GroupDeleted as i32,
        &group.uuid,
        &org_id,
        &UserId::from("00000000-0000-0000-0000-000000000000"),
        0,
        &std::net::IpAddr::from([0, 0, 0, 0]),
        &conn,
    )
    .await;

    group.delete(&conn).await
}

#[get("/public/groups/<group_id>/users")]
async fn public_get_group_members(group_id: GroupId, token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    if !CONFIG.org_groups_enabled() {
        err!("Group support is disabled");
    }

    if Group::find_by_uuid_and_org(&group_id, &org_id, &conn).await.is_none() {
        err!("Group could not be found!", "Group uuid is invalid or does not belong to the organization")
    };

    let group_members: Vec<MembershipId> = GroupUser::find_by_group(&group_id, &conn)
        .await
        .iter()
        .map(|entry| entry.users_organizations_uuid.clone())
        .collect();

    Ok(Json(serde_json::json!(group_members)))
}

#[put("/public/groups/<group_id>/users", data = "<data>")]
async fn public_put_group_members(
    group_id: GroupId,
    data: Json<Vec<MembershipId>>,
    token: PublicToken,
    conn: DbConn,
) -> EmptyResult {
    let org_id = token.0;
    if !CONFIG.org_groups_enabled() {
        err!("Group support is disabled");
    }

    if Group::find_by_uuid_and_org(&group_id, &org_id, &conn).await.is_none() {
        err!("Group could not be found!", "Group uuid is invalid or does not belong to the organization")
    };

    GroupUser::delete_all_by_group(&group_id, &conn).await?;

    let assigned_members = data.into_inner();
    for assigned_member in assigned_members {
        let mut user_entry = GroupUser::new(group_id.clone(), assigned_member.clone());
        user_entry.save(&conn).await?;
    }

    Ok(())
}

// Public API Member Endpoints
#[get("/public/members?<data..>")]
async fn public_get_members(data: PublicGetOrgUserData, token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    let mut users_json = Vec::new();
    for u in Membership::find_by_org(&org_id, &conn).await {
        users_json.push(
            u.to_json_user_details(
                data.include_collections.unwrap_or(false),
                data.include_groups.unwrap_or(false),
                &conn,
            )
            .await,
        );
    }

    Ok(Json(serde_json::json!({
        "data": users_json,
        "object": "list",
        "continuationToken": null,
    })))
}

#[get("/public/members/<member_id>?<data..>")]
async fn public_get_member(
    member_id: MembershipId,
    data: PublicGetOrgUserData,
    token: PublicToken,
    conn: DbConn,
) -> JsonResult {
    let org_id = token.0;
    let Some(user) = Membership::find_by_uuid_and_org(&member_id, &org_id, &conn).await else {
        err!("The specified user isn't a member of the organization")
    };

    let include_groups = data.include_groups.unwrap_or(false);
    Ok(Json(
        user.to_json_user_details(data.include_collections.unwrap_or(include_groups), include_groups, &conn).await
    ))
}

#[post("/public/members/invite", data = "<data>")]
async fn public_post_members_invite(data: Json<PublicInviteData>, token: PublicToken, conn: DbConn) -> EmptyResult {
    let org_id = token.0;
    let data: PublicInviteData = data.into_inner();

    let raw_type = &data.r#type.into_string();
    let new_type = match MembershipType::from_str(raw_type) {
        Some(new_type) => new_type as i32,
        None => err!("Invalid type"),
    };

    let access_all = new_type >= MembershipType::Admin
        || (raw_type.eq("4")
            && data.permissions.get("editAnyCollection") == Some(&serde_json::json!(true))
            && data.permissions.get("deleteAnyCollection") == Some(&serde_json::json!(true))
            && data.permissions.get("createNewCollections") == Some(&serde_json::json!(true)));

    let (org_name, org_email) = match Organization::find_by_uuid(&org_id, &conn).await {
        Some(org) => (org.name, org.billing_email),
        None => err!("Error looking up organization"),
    };

    let mut user_created: bool = false;
    for email in data.emails.iter() {
        let mut member_status = MembershipStatus::Invited as i32;
        let user = match User::find_by_mail(email, &conn).await {
            None => {
                if !CONFIG.invitations_allowed() {
                    err!(format!("User does not exist: {email}"))
                }

                if !CONFIG.is_email_domain_allowed(email) {
                    err!("Email domain not eligible for invitations")
                }

                if !CONFIG.mail_enabled() {
                    Invitation::new(email).save(&conn).await?;
                }

                let mut new_user = User::new(email, None);
                new_user.save(&conn).await?;
                user_created = true;
                new_user
            }
            Some(user) => {
                if Membership::find_by_user_and_org(&user.uuid, &org_id, &conn).await.is_some() {
                    err!(format!("User already in organization: {email}"))
                } else {
                    if !CONFIG.mail_enabled() && !user.password_hash.is_empty() {
                        member_status = MembershipStatus::Accepted as i32;
                    }
                    user
                }
            }
        };

        let mut new_member = Membership::new(user.uuid.clone(), org_id.clone(), Some(org_email.clone()));
        new_member.access_all = access_all;
        new_member.atype = new_type;
        new_member.status = member_status;
        new_member.save(&conn).await?;

        if CONFIG.mail_enabled() {
            if let Err(e) =
                mail::send_invite(&user, org_id.clone(), new_member.uuid.clone(), &org_name, Some(org_email.clone())).await
            {
                if user_created {
                    user.delete(&conn).await?;
                } else {
                    new_member.delete(&conn).await?;
                }

                err!(format!("Error sending invite: {e:?} "));
            }
        }

        // Event logging
        log_event(
            EventType::OrganizationUserInvited as i32,
            &new_member.uuid,
            &org_id,
            &UserId::from("00000000-0000-0000-0000-000000000000"),
            0,
            &std::net::IpAddr::from([0, 0, 0, 0]),
            &conn,
        )
        .await;

        // If no accessAll, add the collections received
        if !access_all {
            for col in data.collections.iter().flatten() {
                match Collection::find_by_uuid_and_org(&col.id, &org_id, &conn).await {
                    None => err!("Collection not found in Organization"),
                    Some(collection) => {
                        CollectionUser::save(
                            &user.uuid,
                            &collection.uuid,
                            col.read_only,
                            col.hide_passwords,
                            col.manage,
                            &conn,
                        )
                        .await?;
                    }
                }
            }
        }

        for group_id in data.groups.iter() {
            let mut group_entry = GroupUser::new(group_id.clone(), new_member.uuid.clone());
            group_entry.save(&conn).await?;
        }
    }

    Ok(())
}

#[put("/public/members/<member_id>", data = "<data>")]
async fn public_put_member(
    member_id: MembershipId,
    data: Json<PublicEditUserData>,
    token: PublicToken,
    conn: DbConn,
) -> EmptyResult {
    let org_id = token.0;
    let data: PublicEditUserData = data.into_inner();

    let raw_type = &data.r#type.into_string();
    let Some(new_type) = MembershipType::from_str(raw_type) else {
        err!("Invalid type")
    };

    let access_all = new_type >= MembershipType::Admin
        || (raw_type.eq("4")
            && data.permissions.get("editAnyCollection") == Some(&serde_json::json!(true))
            && data.permissions.get("deleteAnyCollection") == Some(&serde_json::json!(true))
            && data.permissions.get("createNewCollections") == Some(&serde_json::json!(true)));

    let mut member_to_edit = match Membership::find_by_uuid_and_org(&member_id, &org_id, &conn).await {
        Some(member) => member,
        None => err!("The specified user isn't member of the organization"),
    };

    member_to_edit.access_all = access_all;
    member_to_edit.atype = new_type as i32;

    // Delete all the old collections
    for c in CollectionUser::find_by_organization_and_user_uuid(&org_id, &member_to_edit.user_uuid, &conn).await {
        c.delete(&conn).await?;
    }

    // If no accessAll, add the collections received
    if !access_all {
        for col in data.collections.iter().flatten() {
            match Collection::find_by_uuid_and_org(&col.id, &org_id, &conn).await {
                None => err!("Collection not found in Organization"),
                Some(collection) => {
                    CollectionUser::save(
                        &member_to_edit.user_uuid,
                        &collection.uuid,
                        col.read_only,
                        col.hide_passwords,
                        col.manage,
                        &conn,
                    )
                    .await?;
                }
            }
        }
    }

    GroupUser::delete_all_by_member(&member_to_edit.uuid, &conn).await?;

    for group_id in data.groups.iter().flatten() {
        let mut group_entry = GroupUser::new(group_id.clone(), member_to_edit.uuid.clone());
        group_entry.save(&conn).await?;
    }

    // Event logging
    log_event(
        EventType::OrganizationUserUpdated as i32,
        &member_to_edit.uuid,
        &org_id,
        &UserId::from("00000000-0000-0000-0000-000000000000"),
        0,
        &std::net::IpAddr::from([0, 0, 0, 0]),
        &conn,
    )
    .await;

    member_to_edit.save(&conn).await
}

#[delete("/public/members/<member_id>")]
async fn public_delete_member(member_id: MembershipId, token: PublicToken, conn: DbConn) -> EmptyResult {
    let org_id = token.0;
    let Some(member_to_delete) = Membership::find_by_uuid_and_org(&member_id, &org_id, &conn).await else {
        err!("User to delete isn't member of the organization")
    };

    if member_to_delete.atype == MembershipType::Owner && member_to_delete.status == MembershipStatus::Confirmed as i32 {
        if Membership::count_confirmed_by_org_and_type(&org_id, MembershipType::Owner, &conn).await <= 1 {
            err!("Can't delete the last owner")
        }
    }

    // Event logging
    log_event(
        EventType::OrganizationUserRemoved as i32,
        &member_to_delete.uuid,
        &org_id,
        &UserId::from("00000000-0000-0000-0000-000000000000"),
        0,
        &std::net::IpAddr::from([0, 0, 0, 0]),
        &conn,
    )
    .await;

    member_to_delete.delete(&conn).await
}

// Public API Collection Endpoints
#[get("/public/collections")]
async fn public_get_collections(token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    let collections: Vec<serde_json::Value> = Collection::find_by_organization(&org_id, &conn)
        .await
        .iter()
        .map(|c| c.to_json())
        .collect();

    Ok(Json(serde_json::json!({
        "data": collections,
        "object": "list",
        "continuationToken": null,
    })))
}

#[get("/public/collections/<collection_id>")]
async fn public_get_collection(collection_id: CollectionId, token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    let Some(collection) = Collection::find_by_uuid_and_org(&collection_id, &org_id, &conn).await else {
        err!("Collection not found", "Collection uuid is invalid or does not belong to the organization")
    };

    Ok(Json(collection.to_json()))
}

#[post("/public/collections", data = "<data>")]
async fn public_post_collection(data: Json<PublicCollectionRequest>, token: PublicToken, conn: DbConn) -> JsonResult {
    let org_id = token.0;
    let data: PublicCollectionRequest = data.into_inner();

    let Some(org) = Organization::find_by_uuid(&org_id, &conn).await else {
        err!("Can't find organization details")
    };

    let collection = Collection::new(org.uuid, data.name, data.external_id);
    collection.save(&conn).await?;

    // Event logging
    log_event(
        EventType::CollectionCreated as i32,
        &collection.uuid,
        &org_id,
        &UserId::from("00000000-0000-0000-0000-000000000000"),
        0,
        &std::net::IpAddr::from([0, 0, 0, 0]),
        &conn,
    )
    .await;

    // Add groups
    for group in data.groups {
        CollectionGroup::new(collection.uuid.clone(), group.id, group.read_only, group.hide_passwords, group.manage)
            .save(&conn)
            .await?;
    }

    // Add users
    for user in data.users {
        let Some(member) = Membership::find_by_uuid_and_org(&user.id, &org_id, &conn).await else {
            err!("User is not part of organization")
        };

        if member.access_all {
            continue;
        }

        CollectionUser::save(&member.user_uuid, &collection.uuid, user.read_only, user.hide_passwords, user.manage, &conn)
            .await?;
    }

    // Return collection details (simplified, without user-specific details)
    Ok(Json(collection.to_json()))
}

#[put("/public/collections/<collection_id>", data = "<data>")]
async fn public_put_collection(
    collection_id: CollectionId,
    data: Json<PublicCollectionRequest>,
    token: PublicToken,
    conn: DbConn,
) -> JsonResult {
    let org_id = token.0;
    let data: PublicCollectionRequest = data.into_inner();

    if Organization::find_by_uuid(&org_id, &conn).await.is_none() {
        err!("Can't find organization details")
    };

    let Some(mut collection) = Collection::find_by_uuid_and_org(&collection_id, &org_id, &conn).await else {
        err!("Collection not found")
    };

    collection.name = data.name;
    collection.set_external_id(data.external_id);
    collection.save(&conn).await?;

    // Event logging
    log_event(
        EventType::CollectionUpdated as i32,
        &collection.uuid,
        &org_id,
        &UserId::from("00000000-0000-0000-0000-000000000000"),
        0,
        &std::net::IpAddr::from([0, 0, 0, 0]),
        &conn,
    )
    .await;

    // Update groups
    CollectionGroup::delete_all_by_collection(&collection_id, &conn).await?;
    for group in data.groups {
        CollectionGroup::new(collection_id.clone(), group.id, group.read_only, group.hide_passwords, group.manage)
            .save(&conn)
            .await?;
    }

    // Update users
    CollectionUser::delete_all_by_collection(&collection_id, &conn).await?;
    for user in data.users {
        let Some(member) = Membership::find_by_uuid_and_org(&user.id, &org_id, &conn).await else {
            err!("User is not part of organization")
        };

        if member.access_all {
            continue;
        }

        CollectionUser::save(&member.user_uuid, &collection_id, user.read_only, user.hide_passwords, user.manage, &conn)
            .await?;
    }

    Ok(Json(collection.to_json()))
}

#[delete("/public/collections/<collection_id>")]
async fn public_delete_collection(collection_id: CollectionId, token: PublicToken, conn: DbConn) -> EmptyResult {
    let org_id = token.0;
    let Some(collection) = Collection::find_by_uuid_and_org(&collection_id, &org_id, &conn).await else {
        err!("Collection not found", "Collection does not exist or does not belong to this organization")
    };

    // Event logging
    log_event(
        EventType::CollectionDeleted as i32,
        &collection.uuid,
        &org_id,
        &UserId::from("00000000-0000-0000-0000-000000000000"),
        0,
        &std::net::IpAddr::from([0, 0, 0, 0]),
        &conn,
    )
    .await;

    collection.delete(&conn).await
}
