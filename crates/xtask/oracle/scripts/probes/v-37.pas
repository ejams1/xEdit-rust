{ parity probe: v-37 }
unit P_v_37;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '5' + True;
  Show('''5'' + True', v);
end;
end.
