{ parity probe: x-163 }
unit P_x_163;

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
  v := 'abc'; v := v = ''; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
