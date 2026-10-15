{ parity probe: v-slot-34 }
unit P_v_slot_34;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Round(1.5));
  Show('VarType(Round(1.5))', v);
end;
end.
