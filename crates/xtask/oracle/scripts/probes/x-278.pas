{ parity probe: x-278 }
unit P_x_278;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := Length(sl.Text); AddMessage(IntToStr(v));
  AddMessage('after');
end;
end.
