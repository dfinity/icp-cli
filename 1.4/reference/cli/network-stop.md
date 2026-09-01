# icp network stop

Stop a background network

**Usage:** `icp network stop [OPTIONS] [NAME]`

Examples:

    # Stop default 'local' network
    icp network stop
  
    # Stop explicit network
    icp network stop mynetwork
  
    # Stop using environment flag
    icp network stop -e staging
  
    # Stop using ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network stop
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network stop local


###### **Arguments:**

* `<NAME>` — Name of the network to use.

   Takes precedence over -e/--environment and the ICP_ENVIRONMENT environment variable when specified explicitly.

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Use the network configured in the specified environment.

   Cannot be used together with an explicit network name argument.
   The ICP_ENVIRONMENT environment variable is also checked when neither network name nor -e flag is specified.




