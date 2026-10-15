{ parity probe: v-62 }
unit P_v_62;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := Null + 'x';
  Show('Null + ''x''', v);
end;
end.
