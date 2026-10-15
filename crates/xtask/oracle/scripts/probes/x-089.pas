{ parity probe: x-089 }
unit P_x_089;

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
  AddMessage(IntToStr(Trunc(-2.5)));
  AddMessage('after');
end;
end.
