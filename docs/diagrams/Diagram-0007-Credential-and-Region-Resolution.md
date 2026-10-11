# Credential and Region Resolution (Issue 7)

```mermaid
flowchart TD
    start(["collect with CollectRequest.profile"]) --> p{"--profile given?"}
    p -->|no| cred_d["Credentials: default chain of aws-config (environment variables, shared config and credentials files with AWS_PROFILE, web identity, ECS, IMDSv2)"]
    p -->|yes| cred_p["Credentials: ProfileFileCredentialsProvider for NAME, passed to credentials_provider, and profile_name set to NAME"]
    cred_d --> reg_d["Region for the first API: SDK default region resolution, then us-east-1"]
    cred_p --> reg_p["Region for the first API: AWS_REGION, then AWS_DEFAULT_REGION, then the region of profile NAME, then us-east-1"]
    reg_d --> cfg["ConfigLoader builds SdkConfig"]
    reg_p --> cfg
    cfg --> call["First API call: GetCallerIdentity"]
    call --> fail{"GetCallerIdentity fails? (for example profile missing or no credentials)"}
    fail -->|yes| se(["SetupError"])
    fail -->|no| ok(["Continue with SdkConfig"])

    subgraph notes ["Notes"]
        n_env["With --profile, environment credentials such as AWS_ACCESS_KEY_ID are not consulted"]
        n_name["profile_name alone is not enough: the default chain would check environment credentials first"]
        n_assume["AssumeRole with credential_source = Environment uses environment credentials only when the profile chooses it"]
        n_region["The region is used only for GetCallerIdentity, DescribeRegions and AssumeRole, not as a scan target"]
        n_configure["With --profile, the first-API region is passed to ProfileFileCredentialsProvider with configure, for the AssumeRole STS calls"]
    end
```
