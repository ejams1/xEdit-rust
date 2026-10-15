{ parity probe: v-23 }
unit P_v_23;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := True and 1;
  Show('True and 1', v);
end;
end.
