{ parity probe: v-72 }
unit P_v_72;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '' = Unassigned;
  Show(''''' = Unassigned', v);
end;
end.
