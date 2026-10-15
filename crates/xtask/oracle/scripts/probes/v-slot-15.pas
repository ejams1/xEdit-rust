{ parity probe: v-slot-15 }
unit P_v_slot_15;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  c: Cardinal;
begin
  c := -1;
  v := c;
  Show('c', v);
end;
end.
