{ parity probe: v-70 }
unit P_v_70;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := Unassigned = 0;
  Show('Unassigned = 0', v);
end;
end.
