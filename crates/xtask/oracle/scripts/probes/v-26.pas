{ parity probe: v-26 }
unit P_v_26;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := - 'x';
  Show('- ''x''', v);
end;
end.
