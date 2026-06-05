# icp network ping

Try to connect to a network, and print out its status

**Usage:** `icp network ping [OPTIONS] [NAME]`

Examples:

    # Ping default 'local' network
    icp network ping
  
    # Ping explicit network
    icp network ping mynetwork
  
    # Ping using environment flag
    icp network ping -e staging
  
    # Ping using ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network ping
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network ping local
  
    # Wait until healthy
    icp network ping --wait-healthy


###### **Arguments:**

* `<NAME>` — Name of the network to use.

   Takes precedence over -e/--environment and the ICP_ENVIRONMENT environment variable when specified explicitly.

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Use the network configured in the specified environment.

   Cannot be used together with an explicit network name argument.
   The ICP_ENVIRONMENT environment variable is also checked when neither network name nor -e flag is specified.
* `--wait-healthy` — Repeatedly ping until the replica is healthy or 1 minute has passed




