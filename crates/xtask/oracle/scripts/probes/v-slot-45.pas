{ parity probe: v-slot-45 }
unit P_v_slot_45;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Max(1, 2));
  Show('VarType(Max(1, 2))', v);
end;
end.
