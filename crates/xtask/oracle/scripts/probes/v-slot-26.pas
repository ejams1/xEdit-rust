{ parity probe: v-slot-26 }
unit P_v_slot_26;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  b: Byte;
begin
  b := 300;
  v := b;
  Show('b', v);
end;
end.
