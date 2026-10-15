{ parity probe: v-slot-50 }
unit P_v_slot_50;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Floor(1.5));
  Show('VarType(Floor(1.5))', v);
end;
end.
