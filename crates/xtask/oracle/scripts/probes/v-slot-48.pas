{ parity probe: v-slot-48 }
unit P_v_slot_48;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  sl: TStringList;
begin
  sl := TStringList.Create;
  v := VarType(Length(sl.Text));
  Show('VarType(Length(sl.Text))', v);
end;
end.
