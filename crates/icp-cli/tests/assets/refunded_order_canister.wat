;; A stand-in for CyclePay's backend canister, answering `order_status` (query)
;; for every order id with one fixed reply, Candid-encoded as the real
;; canister sends it (every field of the record, in its order): order
;; `co-1-0000test`, $10.00 to a Cycles Ledger account, paid, not delivered
;; and refunded in full, with the detail "the cycles could not be delivered".
;;
;; Every stand-in also answers, each with one fixed reply Candid-encoded as
;; the real canister sends it: `get_card_terms` (query) with CyclePay's
;; published terms ($1 to $200, 2.9% + $0.30, two Cycles Ledger fees on a
;; delivery, not paused); `get_xdr_rate_status` (query) with $1.3520 an XDR;
;; and `create_order` (update, its argument ignored) with order
;; `co-1-0000test` for $10.00, to be paid on
;; `https://buy.stripe.com/test_standin?client_reference_id=o_co-1-0000test&prefilled_amount=1000`.
;;
;; Rebuild with: wasm-tools parse refunded_order_canister.wat -o refunded_order_canister.wasm
(module
  (import "ic0" "msg_reply_data_append" (func $reply_data_append (param i32 i32)))
  (import "ic0" "msg_reply" (func $reply))
  (memory 1)
  (data (i32.const 0) "\44\49\44\4c\06\6e\01\6c\10\b5\d3\a0\27\02\b2\ce\ef\2f\03\92\c7\e9\57\04\8e\95\ee\8f\01\05\e4\a8\c4\c3\02\02\b3\b0\da\c3\03\68\f0\8c\c4\8d\04\78\aa\ac\d9\d0\06\78\91\c5\fd\80\07\02\b1\ad\cb\e1\09\78\86\bd\da\8b\0b\04\ac\9c\e7\81\0c\71\c1\d9\8f\a4\0c\02\ab\9f\e4\d9\0c\78\de\a7\f7\da\0d\04\df\c5\d8\de\0d\04\6e\71\6b\08\ca\c6\99\a0\01\7f\f7\8d\ab\cb\02\7f\e4\80\bf\9f\05\7f\fa\e9\b0\eb\05\7f\bf\88\9f\83\06\7f\91\ec\f7\db\09\7f\b5\e5\a2\fa\09\7f\f3\bb\99\ef\0c\7f\6e\78\6b\02\c7\bc\99\b4\07\68\c9\b3\ff\e7\0c\68\01\00\01\01\07\70\69\5f\74\65\73\74\01\00\01\01\0a\00\00\00\00\00\00\00\01\01\01\01\07\63\73\5f\74\65\73\74\01\0a\00\00\00\00\00\00\00\01\01\01\ad\03\00\00\00\00\00\00\00\80\c9\6a\00\00\00\00\01\21\74\68\65\20\63\79\63\6c\65\73\20\63\6f\75\6c\64\20\6e\6f\74\20\62\65\20\64\65\6c\69\76\65\72\65\64\e8\03\00\00\00\00\00\00\01\3c\80\c9\6a\00\00\00\00\0d\63\6f\2d\31\2d\30\30\30\30\74\65\73\74\00\e8\03\00\00\00\00\00\00\00\01\e8\03\00\00\00\00\00\00")
  (data (i32.const 1024) "\44\49\44\4c\01\6c\07\bb\c6\db\91\01\7e\93\a6\b4\ba\01\71\86\86\c6\e2\03\78\ac\ba\9d\d1\05\78\f4\8b\ab\8f\0b\78\fb\d3\df\ef\0c\7d\8e\fe\f1\e2\0f\78\01\00\00\23\68\74\74\70\73\3a\2f\2f\62\75\79\2e\73\74\72\69\70\65\2e\63\6f\6d\2f\74\65\73\74\5f\73\74\61\6e\64\69\6e\20\4e\00\00\00\00\00\00\22\01\00\00\00\00\00\00\64\00\00\00\00\00\00\00\80\84\af\5f\1e\00\00\00\00\00\00\00")
  (data (i32.const 1536) "\44\49\44\4c\02\6c\03\ab\c0\cf\b1\05\78\99\de\ad\a0\08\7e\bf\b7\85\fe\0f\01\6e\78\01\00\d0\34\00\00\00\00\00\00\00\00")
  (data (i32.const 2048) "\44\49\44\4c\02\6b\01\bc\8a\01\01\6c\06\c3\a1\80\a5\03\71\f0\8c\c4\8d\04\78\f6\ac\d3\c7\0b\71\ac\9c\e7\81\0c\71\ab\9f\e4\d9\0c\78\de\a7\f7\da\0d\78\01\00\00\0f\6f\5f\63\6f\2d\31\2d\30\30\30\30\74\65\73\74\ad\03\00\00\00\00\00\00\5d\68\74\74\70\73\3a\2f\2f\62\75\79\2e\73\74\72\69\70\65\2e\63\6f\6d\2f\74\65\73\74\5f\73\74\61\6e\64\69\6e\3f\63\6c\69\65\6e\74\5f\72\65\66\65\72\65\6e\63\65\5f\69\64\3d\6f\5f\63\6f\2d\31\2d\30\30\30\30\74\65\73\74\26\70\72\65\66\69\6c\6c\65\64\5f\61\6d\6f\75\6e\74\3d\31\30\30\30\0d\63\6f\2d\31\2d\30\30\30\30\74\65\73\74\e8\03\00\00\00\00\00\00\00\57\86\f4\00\00\00\00")
  (func $order_status
    (call $reply_data_append (i32.const 0) (i32.const 319))
    (call $reply))
  (func $get_card_terms
    (call $reply_data_append (i32.const 1024) (i32.const 124))
    (call $reply))
  (func $get_xdr_rate_status
    (call $reply_data_append (i32.const 1536) (i32.const 39))
    (call $reply))
  (func $create_order
    (call $reply_data_append (i32.const 2048) (i32.const 200))
    (call $reply))
  (export "canister_query get_card_terms" (func $get_card_terms))
  (export "canister_query get_xdr_rate_status" (func $get_xdr_rate_status))
  (export "canister_update create_order" (func $create_order))
  (export "canister_query order_status" (func $order_status)))
