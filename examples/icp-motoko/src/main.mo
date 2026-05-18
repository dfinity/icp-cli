import Prim "mo:prim";

persistent actor {

  transient var env : Text = "";
  for (v in Prim.envVarNames<system>().vals()) {
    env := env # v # ",";
    //      let val = Prim.envVar(v);
    //      env := env # v # "=" # val # "\n";
  };

  public query func greet(name : Text) : async Text {
    return "Hello, " # name # "!";
  };

  public query func getEnv() : async Text { env };

  public func get(name : Text) : async ?Text {
    Prim.envVar<system>(name);
  };
};
