{ parity probe: v-06 }
unit P_v_06;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := True + 1;
  Show('True + 1', v);
end;
end.
