{ parity probe: v-slot-33 }
unit P_v_slot_33;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Trunc(1.5));
  Show('VarType(Trunc(1.5))', v);
end;
end.
