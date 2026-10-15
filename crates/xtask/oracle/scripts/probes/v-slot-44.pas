{ parity probe: v-slot-44 }
unit P_v_slot_44;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Abs(-1));
  Show('VarType(Abs(-1))', v);
end;
end.
