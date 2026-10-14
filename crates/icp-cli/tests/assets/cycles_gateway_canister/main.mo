// A stand-in for a cycles gateway, serving the four methods `icp cycles buy`
// uses with the records CyclePay's backend sends (every field, as its
// Candid declares them), so that the command is tested against a canister on
// a live network rather than a fake in the process.
//
// Terms: $1 to $200 an order, 2.9% + $0.30 a card, two Cycles Ledger fees on
// a delivery to a ledger account, not paused. Rate: $1.3520 an XDR. Every
// `create_order` is order `co-1-0000test` for $10.00, to be paid on
// `https://buy.stripe.com/test_standin?client_reference_id=o_co-1-0000test&prefilled_amount=1000`,
// payable until 2100-01-01T00:00:00Z.
//
// `order_status` reports that order as the stand-in was last told:
// awaiting payment until `pay` or `refund` is called; paid and delivered,
// 6.938T cycles to the canister `pay` names, after `pay`; paid and refunded
// in full, "the cycles could not be delivered", after `refund`.
persistent actor {
  type CardTerms = {
    deliveries_paused : Bool;
    payment_link : Text;
    max_order_usd_cents : Nat64;
    fee_bps : Nat64;
    min_order_usd_cents : Nat64;
    ledger_delivery_fee_cycles : Nat;
    fee_fixed_usd_cents : Nat64;
  };

  type XdrRateSource = { #Xrc; #Legacy; #Manual };

  type XdrRateStatus = {
    last_error : ?Text;
    updated_at : ?Nat64;
    source : XdrRateSource;
    xdr_usd_x10000 : Nat64;
    last_xrc_timestamp : ?Nat64;
    last_error_at : ?Nat64;
    stale : Bool;
    auto_refresh : Bool;
    last_xrc_rate_x10000 : ?Nat64;
    last_attempt_at : ?Nat64;
    consecutive_failures : Nat32;
    refresh_interval_seconds : Nat64;
    age_seconds : ?Nat64;
  };

  type OrderDestination = { #Canister : Principal; #Ledger };

  type Destination = { #Canister : Principal; #Ledger : Principal };

  type CreateOrderArgs = {
    destination : OrderDestination;
    beneficiary : ?Principal;
    project_id : ?Text;
    amount_usd_cents : Nat64;
    confirm_system_canister : ?Bool;
  };

  type CreatedOrder = {
    destination : Destination;
    client_reference_id : Text;
    net_usd_cents : Nat64;
    payment_url : Text;
    order_id : Text;
    amount_usd_cents : Nat64;
    expires_at : Nat64;
  };

  type OrderError = { code : Text; message : Text };

  type CardOrderStatus = {
    #AwaitingPayment;
    #PaymentPending;
    #Processing;
    #Delivered;
    #NeedsReview;
    #Refunding;
    #Refunded;
    #RefundOwed;
  };

  type OrderStatusView = {
    payment_intent : ?Text;
    status : CardOrderStatus;
    cycles_delivered : ?Nat64;
    destination : Destination;
    session_id : ?Text;
    owner : Principal;
    net_usd_cents : Nat64;
    created_at : Nat64;
    detail : ?Text;
    amount_refunded_usd_cents : Nat64;
    paid_at : ?Nat64;
    order_id : Text;
    project_id : ?Text;
    amount_usd_cents : Nat64;
    expires_at : ?Nat64;
    paid_usd_cents : ?Nat64;
  };

  let ORDER_ID = "co-1-0000test";
  let AMOUNT_USD_CENTS : Nat64 = 1000;
  // $10.00 less 2.9% + $0.30.
  let NET_USD_CENTS : Nat64 = 941;
  // 2100-01-01T00:00:00Z.
  let EXPIRES_AT : Nat64 = 4102444800;
  let PAYMENT_LINK = "https://buy.stripe.com/test_standin";
  let CYCLES_DELIVERED : Nat64 = 6_938_000_000_000;
  let PAID_AT : Nat64 = 1;

  // Where the order stands: as made, paid and delivered to a canister, or
  // refunded.
  var outcome : { #awaiting; #delivered : Principal; #refunded } = #awaiting;

  public query func get_card_terms() : async CardTerms {
    {
      deliveries_paused = false;
      payment_link = PAYMENT_LINK;
      max_order_usd_cents = 20_000;
      fee_bps = 290;
      min_order_usd_cents = 100;
      ledger_delivery_fee_cycles = 200_000_000;
      fee_fixed_usd_cents = 30;
    };
  };

  public query func get_xdr_rate_status() : async XdrRateStatus {
    {
      last_error = null;
      updated_at = ?0;
      source = #Manual;
      xdr_usd_x10000 = 13_520;
      last_xrc_timestamp = null;
      last_error_at = null;
      stale = false;
      auto_refresh = false;
      last_xrc_rate_x10000 = null;
      last_attempt_at = null;
      consecutive_failures = 0;
      refresh_interval_seconds = 3600;
      age_seconds = ?0;
    };
  };

  // The argument is ignored: every order is the one order.
  public shared ({ caller }) func create_order(_args : CreateOrderArgs) : async {
    #Ok : CreatedOrder;
    #Err : OrderError;
  } {
    let order : CreatedOrder = {
      destination = #Ledger(caller);
      client_reference_id = "o_" # ORDER_ID;
      net_usd_cents = NET_USD_CENTS;
      payment_url = PAYMENT_LINK # "?client_reference_id=o_" # ORDER_ID # "&prefilled_amount=1000";
      order_id = ORDER_ID;
      amount_usd_cents = AMOUNT_USD_CENTS;
      expires_at = EXPIRES_AT;
    };
    #Ok(order);
  };

  // Every order id is the one order; the caller is its owner.
  public shared query ({ caller }) func order_status(_id : Text) : async ?OrderStatusView {
    let base : OrderStatusView = {
      payment_intent = null;
      status = #AwaitingPayment;
      cycles_delivered = null;
      destination = #Ledger(caller);
      session_id = null;
      owner = caller;
      net_usd_cents = NET_USD_CENTS;
      created_at = 0 : Nat64;
      detail = null;
      amount_refunded_usd_cents = 0 : Nat64;
      paid_at = null;
      order_id = ORDER_ID;
      project_id = null;
      amount_usd_cents = AMOUNT_USD_CENTS;
      expires_at = ?EXPIRES_AT;
      paid_usd_cents = null;
    };
    switch outcome {
      case (#awaiting) ?base;
      case (#delivered(canister)) {
        let view : OrderStatusView = {
          base with
          status = #Delivered;
          cycles_delivered = ?CYCLES_DELIVERED;
          destination = #Canister(canister);
          paid_at = ?PAID_AT;
          paid_usd_cents = ?AMOUNT_USD_CENTS;
        };
        ?view;
      };
      case (#refunded) {
        let view : OrderStatusView = {
          base with
          status = #Refunded;
          detail = ?"the cycles could not be delivered";
          amount_refunded_usd_cents = AMOUNT_USD_CENTS;
          paid_at = ?PAID_AT;
          paid_usd_cents = ?AMOUNT_USD_CENTS;
        };
        ?view;
      };
    };
  };

  // The card was charged and the cycles delivered to `canister`.
  public func pay(canister : Principal) : async () {
    outcome := #delivered(canister);
  };

  // The card was charged, the delivery failed, and the payment was refunded.
  public func refund() : async () {
    outcome := #refunded;
  };
};
