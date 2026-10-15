{ parity probe: v-32 }
unit P_v_32;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := '10' > '9';
  Show('''10'' > ''9''', v);
end;
end.
