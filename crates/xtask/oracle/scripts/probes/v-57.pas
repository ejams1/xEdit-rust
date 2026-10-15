{ parity probe: v-57 }
unit P_v_57;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 1 div 0;
  Show('1 div 0', v);
end;
end.
