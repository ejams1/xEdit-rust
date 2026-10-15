{ parity probe: v-slot-55 }
unit P_v_slot_55;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  b: Byte;
begin
  b := 5;
  v := VarType(b);
  Show('VarType(b)', v);
end;
end.
