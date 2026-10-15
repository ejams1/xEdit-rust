{ parity probe: v-slot-31 }
unit P_v_slot_31;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := VarType(Length('ab'));
  Show('VarType(Length(''ab''))', v);
end;
end.
