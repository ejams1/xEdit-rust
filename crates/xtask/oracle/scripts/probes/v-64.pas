{ parity probe: v-64 }
unit P_v_64;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := ' 1' + 1;
  Show(''' 1'' + 1', v);
end;
end.
