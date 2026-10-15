{ parity probe: v-66 }
unit P_v_66;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '$10' + 1;
  Show('''$10'' + 1', v);
end;
end.
