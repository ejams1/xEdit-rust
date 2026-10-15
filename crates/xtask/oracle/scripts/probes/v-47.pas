{ parity probe: v-47 }
unit P_v_47;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := 5 or True;
  Show('5 or True', v);
end;
end.
