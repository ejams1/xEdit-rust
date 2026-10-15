{ parity probe: v-36 }
unit P_v_36;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := True + 1.5;
  Show('True + 1.5', v);
end;
end.
