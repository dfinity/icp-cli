# icp network start

Run a given network.

The gateway binds to port 8000 by default. To use a different port, set `gateway.port` in `icp.yaml`. If port 8000 is already in use by another icp-cli project, stop that network first:

icp network stop --project-root-override <path>

**Usage:** `icp network start [OPTIONS] [NAME]`

Examples:

    # Use default 'local' network
    icp network start
  
    # Use explicit network name
    icp network start mynetwork
  
    # Use environment flag
    icp network start -e staging
  
    # Use ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network start
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network start local
  
    # Background mode with environment
    icp network start -e staging -d


###### **Arguments:**

* `<NAME>` — Name of the network to use.

   Takes precedence over -e/--environment and the ICP_ENVIRONMENT environment variable when specified explicitly.

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Use the network configured in the specified environment.

   Cannot be used together with an explicit network name argument.
   The ICP_ENVIRONMENT environment variable is also checked when neither network name nor -e flag is specified.
* `-d`, `--background` — Starts the network in a background process. This command will exit once the network is running. To stop the network, use 'icp network stop'




