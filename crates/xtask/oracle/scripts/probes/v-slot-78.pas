{ parity probe: v-slot-78 }
unit P_v_slot_78;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  s: string;
begin
  v := VarType(s);
  Show('VarType(s)', v);
end;
end.
