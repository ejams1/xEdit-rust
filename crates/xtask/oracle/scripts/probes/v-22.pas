{ parity probe: v-22 }
unit P_v_22;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := Unassigned + 1;
  Show('Unassigned + 1', v);
end;
end.
